use crate::blocks::tokens::gather_inputs;
use crate::blocks::tokens::var_to_tokens;
use crate::blocks::ContractUser;
use crate::blocks::{ContractData, EthData, NodeData};
use crate::blocks::{CONTRACT_TYPE, CONTRACT_VAR, NODE_TYPE};
use chainblocks::block::Block;
use chainblocks::cblog;
use chainblocks::cbstr;
use chainblocks::core::do_blocking;
use chainblocks::core::log;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::Context;
use chainblocks::types::ExposedInfo;
use chainblocks::types::ExposedTypes;
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::RawString;
use chainblocks::types::Table;
use chainblocks::types::Type;
use chainblocks::types::Types;
use chainblocks::types::Var;
use secp256k1::SecretKey;
use std::convert::TryInto;
use std::ffi::CStr;
use std::ffi::CString;
use std::fs;
use web3::contract::Options;
use web3::signing::SecretKeyRef;
use web3::types::Address;
use web3::types::U256;
use zeroize::Zeroize;

pub struct Write {
  cu: ContractUser,
  confirmations: usize,
  options: ParamVar,
  output: Table,
}

static TRANSACTION_TABLE_TYPES: &'static [Type] = &[common_type::bytes, common_type::int];
const TRANSACTION_TABLE_KEYS: &[RawString] =
  &[cbstr!("transaction_hash"), cbstr!("transaction_index")];
static TRANSACTION_TABLE_TYPE: Type = Type::table(TRANSACTION_TABLE_KEYS, TRANSACTION_TABLE_TYPES);

lazy_static! {
  static ref INPUT_TYPES: Vec<Type> = vec![common_type::anys];
  static ref OUTPUT_TYPES: Vec<Type> = vec![TRANSACTION_TABLE_TYPE];
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
      cstr!("The sender secret key or an unlocked account's public key. In the case of a secret key, using a file is safer as it won't be kept in memory."),
      vec![
        common_type::path,
        common_type::path_var,
        common_type::string,
        common_type::string_var,
      ],
    )
      .into(),
    (
      cstr!("Confirmations"),
      cstr!("The amount of confirmations required."),
      vec![common_type::int],
    )
      .into(),
    (
      cstr!("Options"),
      cstr!("Various options to add to this call. (avail: gas, gas-price, value, nonce)"),
      vec![common_type::none, common_type::bytes_table, common_type::bytes_table_var],
    )
      .into()
  ];
}

enum Caller {
  PrivateKey(SecretKey),
  PublicKey(Address),
}

impl Default for Write {
  fn default() -> Self {
    Write {
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
      confirmations: 12,
      options: ParamVar::new(().into()),
      output: Table::new(),
    }
  }
}

impl Write {
  async fn activate_async<'a>(
    data: &EthData,
    from: Caller,
    confirmations: usize,
    input: &Var,
    options: Option<Table>,
    output: &mut Table,
  ) -> Result<(), &'a str> {
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

    // no timeout here as we deal with moneys likely... but TODO check if there is an internal timeout
    // let timed_fut = timeout(Duration::from_secs(30), fut);

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

    let transaction = {
      match from {
        Caller::PrivateKey(key) => {
          let key_ref = SecretKeyRef::new(&key);

          let fut = contract.contract.signed_call_with_confirmations(
            method,
            // notice as_slice is necessary to make the "into" jigsaw fall into pieces
            tokens.as_slice(),
            opts,
            confirmations,
            key_ref,
          );

          fut.await.or_else(|e| {
            cblog!("web3 error: {}", e);
            Err("Write failed")
          })?
        }
        Caller::PublicKey(from) => {
          let fut = contract.contract.call_with_confirmations(
            method,
            // notice as_slice is necessary to make the "into" jigsaw fall into pieces
            tokens.as_slice(),
            from,
            opts,
            confirmations,
          );
          fut.await.or_else(|e| {
            cblog!("web3 error: {}", e);
            Err("Write failed")
          })?
        }
      }
    };

    output.insert_fast_static(
      cstr!("transaction_hash"),
      transaction.transaction_hash.as_bytes().into(),
    );

    output.insert_fast_static(
      cstr!("transaction_index"),
      transaction.transaction_index.as_u64().try_into()?,
    );

    if let Some(block_hash) = transaction.block_hash {
      output.insert_fast_static(cstr!("block_hash"), block_hash.as_bytes().into());
    }

    if let Some(block_number) = transaction.block_number {
      output.insert_fast_static(cstr!("block_number"), block_number.as_u64().try_into()?);
    }

    if let Some(gas_used) = transaction.gas_used {
      let bytes: [u8; 32] = gas_used.into();
      output.insert_fast_static(cstr!("gas_used"), (&bytes[..]).into());
    }

    if let Some(status) = transaction.status {
      output.insert_fast_static(cstr!("status"), status.as_u64().try_into()?);
    }

    // TODO, add logs vector

    Ok(())
  }
}

impl Block for Write {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.Write-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.Write")
  }

  fn name(&mut self) -> &str {
    "Eth.Write"
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
      3 => self.confirmations = value.try_into().unwrap_or(12),
      4 => self.options.setParam(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.cu.instance.getParam(),
      1 => self.cu.data.method.as_ref().into(),
      2 => self.cu.from.getParam(),
      3 => self
        .confirmations
        .try_into()
        .expect("a proper int var, mitigared in setParam, fixme"),
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

      let caller = {
        let from: Result<Address, &str> = {
          let from = &self.cu.from.get();
          if let Ok(s) = TryInto::<String>::try_into(from) {
            if s.len() > 0 {
              if s.starts_with("0x") {
                let subs: &str = &s[2..];
                subs
                  .parse()
                  .or_else(|_| Err("Failed to parse From address"))
              } else {
                s.parse().or_else(|_| Err("Failed to parse From address"))
              }
            } else {
              Err("Expected a publickey, got an empty string")
            }
          } else {
            Err("Expected a publickey, got an invalid string")
          }
        };
        if let Ok(from) = from {
          Caller::PublicKey(from)
        } else {
          let from_key = {
            let from = &self.cu.from.get();
            if let Ok(mut s) = TryInto::<String>::try_into(from) {
              if from.is_path() {
                let mut data = {
                  let mut key_str =
                    fs::read_to_string(s).or_else(|_| Err("Failed to read key file"))?;
                  let key_slice = if key_str.starts_with("0x") {
                    &key_str[2..]
                  } else {
                    &key_str[..]
                  };
                  let bytes = hex::decode(key_slice).or_else(|_| Err("Failed to decode key"))?;
                  key_str.zeroize();
                  bytes
                };
                let key = SecretKey::from_slice(data.as_slice())
                  .or_else(|_| Err("Failed to create SecretKey from file contents"))?;
                data.zeroize();
                Ok(key)
              } else {
                let key_slice = if s.starts_with("0x") { &s[2..] } else { &s[..] };
                let mut bytes = hex::decode(key_slice).or_else(|_| Err("Failed to decode key"))?;
                let key = SecretKey::from_slice(bytes.as_slice())
                  .or_else(|_| Err("Failed to create SecretKey from string"))?;
                bytes.zeroize();
                s.zeroize();
                Ok(key)
              }
            } else {
              Err("SecretKey parameter is invalid")
            }
          }?;
          Caller::PrivateKey(from_key)
        }
      };

      let options: Option<Table> = {
        let optvar = self.options.get();
        if optvar.is_none() {
          None
        } else {
          Some(optvar.as_ref().try_into()?)
        }
      };

      node.scheduler.block_on(Write::activate_async(
        &mut self.cu.data,
        caller,
        self.confirmations,
        input,
        options,
        &mut self.output,
      ))?;
      Ok((&self.output).into())
    }))
  }
}
