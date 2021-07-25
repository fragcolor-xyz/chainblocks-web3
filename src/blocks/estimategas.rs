use crate::blocks::get_timeout;
use crate::blocks::tokens::gather_inputs;
use crate::blocks::tokens::var_to_tokens;
use crate::blocks::ContractUser;
use crate::blocks::{ContractData, EthData, NodeData};
use crate::blocks::{CONTRACT_TYPE, CONTRACT_VAR, NODE_TYPE};
use chainblocks::block::Block;
use chainblocks::cblog;
use chainblocks::core::do_blocking;
use chainblocks::core::log;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::Context;
use chainblocks::types::ExposedInfo;
use chainblocks::types::ExposedTypes;
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::Type;
use chainblocks::types::Types;
use chainblocks::types::{ClonedVar, Var};
use std::convert::TryInto;
use std::ffi::CString;
use std::str;
use std::time::Duration;
use tokio::time::timeout;
use web3::contract::Error;
use web3::contract::Options;
use web3::types::{Address, U256};

pub struct EstimateGas {
  cu: ContractUser,
  timeout: Duration,
  output: ClonedVar,
}

lazy_static! {
  static ref INPUT_TYPES: Vec<Type> = vec![common_type::anys];
  static ref OUTPUT_TYPES: Vec<Type> = vec![common_type::bytes];
  static ref PARAMETERS: Parameters = vec![
    (
      cstr!("Contract"),
      cstr!("The contract instance we operate."),
      vec![CONTRACT_VAR],
    )
      .into(),
    (
      cstr!("Method"),
      cstr!("The method of the contract to call."),
      vec![common_type::string],
    )
      .into(),
    (
      cstr!("From"),
      cstr!("The address we are calling from."),
      vec![common_type::string, common_type::string_var],
    )
      .into()
  ];
}

impl Default for EstimateGas {
  fn default() -> Self {
    EstimateGas {
      cu: ContractUser {
        instance: ParamVar::new(Var::context_variable(cstr!("default.Eth.Contract"))),
        from: ParamVar::new(cstr!("").into()), // none is not supported in this block
        data: EthData {
          contract: None,
          method: CString::new("").unwrap(),
          from: Some(CString::new("").unwrap()),
          input_types: Vec::new(),
        },
        node: None,
        requiring: Vec::new(),
      },
      timeout: get_timeout(),
      output: ClonedVar(Var::default()),
    }
  }
}

impl EstimateGas {
  async fn activate_async<'a>(
    data: &EthData,
    input: &Var,
    timeout_: Duration,
  ) -> Result<U256, &'a str> {
    let method = data.method.to_str().or_else(|_| Err("Invalid string"))?;
    let tokens = var_to_tokens(input, &data.input_types)?;
    let contract_a = data
      .contract
      .as_ref()
      .ok_or_else(|| "Failed to unwrap contract data shared pointer")?;
    let contract_o = &**contract_a;
    let contract = contract_o
      .as_ref()
      .ok_or_else(|| "Failed to unwrap contract data, was empty")?;

    let from: Address = {
      if let Some(from_str) = &data.from {
        let s = from_str.to_str().or_else(|_| Err("Invalid string"))?;
        if s.len() > 0 {
          if s.starts_with("0x") {
            let subs: &str = &s[2..];
            subs
              .parse()
              .or_else(|_| Err("Failed to parse From address"))?
          } else {
            s.parse().or_else(|_| Err("Failed to parse From address"))?
          }
        } else {
          return Err("EstimateGas requires a From address");
        }
      } else {
        return Err("EstimateGas requires a From address");
      }
    };

    let fut = contract.contract.estimate_gas(
      method,
      // notice as_slice is necessary to make the "into" jigsaw fall into pieces
      tokens.as_slice(),
      from,
      Options::default(),
    );
    let timed_fut = timeout(timeout_, fut);
    let result: Result<U256, Error> = timed_fut.await.or_else(|_| Err("RPC request timed out"))?;
    result.or_else(|e| {
      cblog!("query error: {}", e);
      Err("Failed to estimate_gas")
    })
  }
}

impl Block for EstimateGas {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.EstimateGas-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.EstimateGas")
  }

  fn name(&mut self) -> &str {
    "Eth.EstimateGas"
  }
  fn inputTypes(&mut self) -> &Types {
    &INPUT_TYPES
  }
  fn outputTypes(&mut self) -> &Types {
    &OUTPUT_TYPES
  }

  fn parameters(&mut self) -> Option<&Parameters> {
    Some(&PARAMETERS)
  }

  fn setParam(&mut self, index: i32, value: &Var) {
    match index {
      0 => self.cu.instance.set_param(value),
      1 => self.cu.data.method = value.try_into().unwrap_or(CString::new("").unwrap()),
      2 => self.cu.from.set_param(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.cu.instance.get_param(),
      1 => self.cu.data.method.as_ref().into(),
      2 => self.cu.from.get_param(),
      _ => Var::default(),
    }
  }

  fn requiredVariables(&mut self) -> Option<&ExposedTypes> {
    self.cu.requiring.clear();
    let exp_info = ExposedInfo {
      exposedType: CONTRACT_TYPE,
      name: (&self.cu.instance.parameter.0).try_into().unwrap(),
      help: cstr!("The required ethereum contract to use.").into(),
      ..ExposedInfo::default()
    };
    self.cu.requiring.push(exp_info);
    Some(&self.cu.requiring)
  }

  fn warmup(&mut self, context: &Context) -> Result<(), &str> {
    if !self.cu.instance.is_variable() {
      return Err("Contract instance is empty or not valid");
    }

    self.cu.instance.warmup(context);
    self.cu.from.warmup(context);

    Ok(())
  }

  fn cleanup(&mut self) {
    self.cu.instance.cleanup();
    self.cu.from.cleanup();
    self.cu.node = None;
    self.cu.data.contract = None;
  }

  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    if self.cu.data.contract.is_none() {
      self.cu.data.contract = Some(Var::from_object_as_clone::<Option<ContractData>>(
        self.cu.instance.get(),
        &CONTRACT_TYPE,
      )?);

      self.cu.data.from = (&self.cu.from.get()).try_into()?;
      let contract = Var::get_mut_from_clone(&self.cu.data.contract)?;
      // also init input_types from jsonF
      let method = self
        .cu
        .data
        .method
        .to_str()
        .or_else(|_| Err("Invalid string"))?;
      self.cu.data.input_types = gather_inputs(method, &contract.json_abi)?;
      // grab node from contract as well
      self.cu.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        contract.node,
        &NODE_TYPE,
      )?);
    }

    Ok(do_blocking(context, || -> Result<Var, &str> {
      let node = Var::get_mut_from_clone(&self.cu.node)?;
      let res = node.scheduler.block_on(EstimateGas::activate_async(
        &mut self.cu.data,
        input,
        self.timeout,
      ))?;
      let ubits: [u8; 32] = res.into();
      let sbits = &ubits[..];
      self.output = sbits.into();
      Ok(self.output.0)
    }))
  }
}
