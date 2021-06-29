use crate::blocks::get_timeout;
use crate::blocks::tokens::gather_inputs;
use crate::blocks::tokens::tokens_to_var;
use crate::blocks::tokens::var_to_tokens;
use crate::blocks::tokens::MyTokens;
use crate::blocks::ContractUser;
use crate::blocks::Transport;
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
use chainblocks::types::Seq;
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
use web3::contract::Options;
use web3::transports::Batch;
use web3::types::Address;
use web3::types::BlockId;
use web3::types::BlockNumber;
use web3::types::Bytes;
use web3::types::CallRequest;
use web3::types::U256;

pub struct ReadBatch {
  cu: ContractUser,
  block: Option<BlockId>,
  timeout: Duration,
  options: ParamVar,
  output: Vec<ClonedVar>,
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

impl Default for ReadBatch {
  fn default() -> Self {
    ReadBatch {
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
      output: Vec::new(),
    }
  }
}

impl ReadBatch {
  async fn activate_async<'a>(
    data: &EthData,
    input: &Var,
    block: Option<BlockId>,
    timeout_: Duration,
    options: Option<Table>,
    transport: Batch<&Transport>,
  ) -> Result<Vec<MyTokens>, &'a str> {
    let method = data.method.to_str().or_else(|_| Err("Invalid string"))?;
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

    let contract_addr = contract.contract.address();
    let web3 = web3::Web3::new(transport.clone());
    let func = contract.contract.abi().function(method).or_else(|e| {
      cblog!("web3 error: {}", e);
      Err("Could not fetch function from contracts' abi")
    })?;
    if let Ok(datas) = Seq::try_from(input) {
      let mut results = Vec::new();
      for single in datas {
        let tokens = var_to_tokens(&single, &data.input_types)?;
        let encoded = func.encode_input(&tokens).or_else(|e| {
          cblog!("web3 error: {}", e);
          Err("Failed to encode input")
        })?;
        let req = CallRequest {
          from: from.into(),
          to: Some(contract_addr),
          gas: opts.gas,
          gas_price: opts.gas_price,
          value: opts.value,
          data: Some(Bytes(encoded)),
          transaction_type: None, // I think this is one of the latest EIPs/updates, might be useful
          access_list: None,
        };
        results.push(web3.eth().call(req, block));
      }
      let fut = transport.submit_batch();
      let timed_fut = timeout(timeout_, fut);
      timed_fut
        .await
        .or_else(|_| Err("Batch request timed out"))?
        .or_else(|e| {
          cblog!("web3 error: {}", e);
          Err("Failed to execute batch")
        })?;
      let mut vars = Vec::new();
      for result in results {
        let bytes = result.await.or_else(|e| {
          cblog!("web3 error: {}", e);
          Err("A batch operation has failed")
        })?;
        let output = func.decode_output(&bytes.0).or_else(|e| {
          cblog!("web3 error: {}", e);
          Err("A batch operation has failed")
        })?;
        vars.push(MyTokens(output));
      }
      Ok(vars)
    } else {
      Err("Invalid input Var")
    }
  }
}

impl Block for ReadBatch {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.ReadBatch-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.ReadBatch")
  }

  fn name(&mut self) -> &str {
    "Eth.ReadBatch"
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

      // also init input_types from json
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
      let (scheduler, t) = (&mut node.scheduler, node.web3.transport());

      let bt = Batch::new(t);

      let options: Option<Table> = {
        let optvar = self.options.get();
        if optvar.is_none() {
          None
        } else {
          Some(optvar.as_ref().try_into()?)
        }
      };

      let tokens_seq = scheduler.block_on(ReadBatch::activate_async(
        &mut self.cu.data,
        input,
        self.block,
        self.timeout,
        options,
        bt,
      ))?;
      self.output.clear();
      for tokens in tokens_seq {
        let mut v = ClonedVar(Var::default());
        if let Err(error) = tokens_to_var(tokens, &mut v) {
          return Err(error);
        } else {
          self.output.push(v);
        }
      }
      Ok((&self.output).into())
    }))
  }
}
