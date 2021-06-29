use crate::blocks::get_timeout;
use crate::blocks::tokens::gather_inputs;
use crate::blocks::tokens::tokens_to_var;
use crate::blocks::tokens::var_to_tokens;
use crate::blocks::tokens::MyTokens;
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
use chainblocks::types::Table;
use chainblocks::types::Types;
use chainblocks::types::{ClonedVar, Var};
use std::convert::TryFrom;
use std::convert::TryInto;
use std::ffi::CStr;
use std::ffi::CString;
use std::str;
use std::time::Duration;
use tokio::time::timeout;
use web3::contract::Error;
use web3::contract::Options;
use web3::types::Address;
use web3::types::BlockId;
use web3::types::BlockNumber;
use web3::types::U256;

pub struct Read {
  cu: ContractUser,
  block: Option<BlockId>,
  timeout: Duration,
  options: ParamVar,
  output: ClonedVar,
}

lazy_static! {
  static ref INPUT_TYPES: Types = vec![common_type::anys, common_type::none];
  static ref OUTPUT_TYPES: Types = vec![common_type::bytezs];
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
      cstr!("The optional address we are calling from"),
      vec![
        common_type::none,
        common_type::string,
        common_type::string_var,
      ],
    )
      .into(),
    (
      cstr!("Block"),
      cstr!("The optional block number to read from history."),
      vec![common_type::none, common_type::int],
    )
      .into(),
    (
      cstr!("Options"),
      cstr!("Various options to add to this call. (avail: gas, gas-price, value, nonce)"),
      vec![
        common_type::none,
        common_type::bytes_table,
        common_type::bytes_table_var
      ],
    )
      .into()
  ];
}

impl Default for Read {
  fn default() -> Self {
    Read {
      cu: ContractUser {
        instance: ParamVar::new(Var::context_variable(cstr!("default.Eth.Contract"))),
        from: ParamVar::new(Var::default()),
        data: EthData {
          contract: None,
          method: CString::new("").unwrap(),
          from: None,
          input_types: Vec::new(),
        },
        node: None,
        requiring: Vec::new(),
      },
      block: None,
      timeout: get_timeout(),
      options: ParamVar::new(().into()),
      output: ClonedVar(Var::default()),
    }
  }
}

impl Read {
  async fn activate_async<'a>(
    data: &EthData,
    input: &Var,
    block: Option<BlockId>,
    timeout_: Duration,
    options: Option<Table>,
  ) -> Result<MyTokens, &'a str> {
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

    let from: Option<Address> = {
      if let Some(from_str) = &data.from {
        let s = from_str.to_str().or_else(|_| Err("Invalid string"))?;
        if s.len() > 0 {
          if s.starts_with("0x") {
            let subs: &str = &s[2..];
            Some(
              subs
                .parse()
                .or_else(|_| Err("Failed to parse From address"))?,
            )
          } else {
            Some(s.parse().or_else(|_| Err("Failed to parse From address"))?)
          }
        } else {
          None
        }
      } else {
        None
      }
    };

    let opts = {
      if let Some(options) = options {
        let mut opts = Options::default();
        for (key, value) in options.iter() {
          let key = unsafe { CStr::from_ptr(key.0) };
          let key = key.to_str().unwrap();
          match key {
            "gas" => {
              let slice: &[u8] = value.as_ref().try_into()?;
              let u: U256 = slice.into();
              opts.gas = Some(u);
            }
            "gas-price" => {
              let slice: &[u8] = value.as_ref().try_into()?;
              let u: U256 = slice.into();
              opts.gas_price = Some(u);
            }
            "value" => {
              let slice: &[u8] = value.as_ref().try_into()?;
              let u: U256 = slice.into();
              opts.value = Some(u);
            }
            "nonce" => {
              let slice: &[u8] = value.as_ref().try_into()?;
              let u: U256 = slice.into();
              opts.nonce = Some(u);
            }
            _ => {
              cblog!("Ignored an invalid option label: {}", key);
            }
          }
        }
        opts
      } else {
        Options::default()
      }
    };

    let fut = contract.contract.query(
      method,
      // notice as_slice is necessary to make the "into" jigsaw fall into pieces
      tokens.as_slice(),
      from,
      opts,
      block,
    );
    let timed_fut = timeout(timeout_, fut);
    let result: Result<MyTokens, Error> =
      timed_fut.await.or_else(|_| Err("RPC request timed out"))?;
    result.or_else(|e| {
      cblog!("query error: {}", e);
      Err("Failed to detokenize into Var")
    })
  }
}

impl Block for Read {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.Read-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.Read")
  }

  fn name(&mut self) -> &str {
    "Eth.Read"
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
      0 => self.cu.instance.setParam(value),
      1 => self.cu.data.method = value.try_into().unwrap_or(CString::new("").unwrap()),
      2 => self.cu.from.setParam(value),
      3 => {
        if value.is_none() {
          self.block = None;
        } else {
          if let Ok(nblock) = u64::try_from(value) {
            self.block = Some(BlockId::Number(nblock.into()));
          } else {
            self.block = None;
          }
        }
      }
      4 => self.options.setParam(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.cu.instance.getParam(),
      1 => self.cu.data.method.as_ref().into(),
      2 => self.cu.from.getParam(),
      3 => {
        if let Some(blockid) = self.block {
          match blockid {
            BlockId::Number(n) => match n {
              BlockNumber::Number(nn) => nn.as_u64().try_into().unwrap_or(Var::default()),
              _ => unreachable!(),
            },
            _ => unreachable!(),
          }
        } else {
          Var::default()
        }
      }
      4 => self.options.getParam(),
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
    if !self.cu.instance.isVariable() {
      return Err("Contract instance is empty or not valid");
    }

    self.cu.instance.warmup(context);
    self.cu.from.warmup(context);
    self.options.warmup(context);

    Ok(())
  }

  fn cleanup(&mut self) {
    self.options.cleanup();
    self.cu.from.cleanup();
    self.cu.instance.cleanup();

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
      let method = self
        .cu
        .data
        .method
        .to_str()
        .or_else(|_| Err("Invalid string"))?;
      self.cu.data.input_types = gather_inputs(method, &contract.json_abi)?;
      // also populate node data here
      self.cu.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        contract.node,
        &NODE_TYPE,
      )?);
    }

    Ok(do_blocking(context, || -> Result<Var, &str> {
      let node = Var::get_mut_from_clone(&self.cu.node)?;

      let options: Option<Table> = {
        let optvar = self.options.get();
        if optvar.is_none() {
          None
        } else {
          Some(optvar.as_ref().try_into()?)
        }
      };

      let tokens = node.scheduler.block_on(Read::activate_async(
        &mut self.cu.data,
        input,
        self.block,
        self.timeout,
        options,
      ))?;
      match tokens_to_var(tokens, &mut self.output) {
        Err(error) => Err(error),
        Ok(()) => Ok(self.output.0),
      }
    }))
  }
}
