use crate::blocks::get_timeout;
use crate::blocks::NodeData;
use crate::blocks::NODE_TYPE;
use crate::blocks::NODE_VAR;
use chainblocks::block::Block;
use chainblocks::cblog;
use chainblocks::cbstr;
use chainblocks::core::activate_blocking;
use chainblocks::core::log;
use chainblocks::core::BlockingBlock;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::Context;
use chainblocks::types::ExposedInfo;
use chainblocks::types::ExposedTypes;
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::RawString;
use chainblocks::types::Seq;
use chainblocks::types::Table;
use chainblocks::types::Type;
use chainblocks::types::Var;
use std::convert::TryInto;
use std::rc::Rc;
use std::str;
use std::time::Duration;
use tokio::time::timeout;
use web3::types::BlockId;

static TABLE_TYPES: &'static [Type] = &[
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytezs,
];
static TABLE_TYPES2: &'static [Type] = &[
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  crate::blocks::TX_TABLE_TYPE,
];
const TABLE_KEYS: &[RawString] = &[
  cbstr!("parent_hash"),
  cbstr!("uncles_hash"),
  cbstr!("author"),
  cbstr!("state_root"),
  cbstr!("transactions_root"),
  cbstr!("receipts_root"),
  cbstr!("gas_used"),
  cbstr!("gas_limit"),
  cbstr!("timestamp"),
  cbstr!("difficulty"),
  cbstr!("transactions"),
];
static TABLE_TYPE: Type = Type::table(TABLE_KEYS, TABLE_TYPES);
static TABLE_TYPE2: Type = Type::table(TABLE_KEYS, TABLE_TYPES2);

pub struct EthBlock {
  full: bool,
  node_param: ParamVar,
  node: Option<Rc<Option<NodeData>>>,
  output: Table,
  timeout: Duration,
  requiring: ExposedTypes,
}

impl Default for EthBlock {
  fn default() -> Self {
    EthBlock {
      full: false,
      node_param: ParamVar::new(Var::context_variable(cstr!("default.Eth"))),
      node: None,
      output: Table::new(),
      timeout: get_timeout(),
      requiring: Vec::new(),
    }
  }
}

lazy_static! {
  static ref INPUT_TYPES: Vec<Type> = vec![common_type::int];
  static ref OUTPUT_TYPES: Vec<Type> = vec![TABLE_TYPE];
  static ref OUTPUT_TYPES2: Vec<Type> = vec![TABLE_TYPE2];
  static ref PARAMETERS: Parameters = vec![
    (
      cstr!("Full"),
      cstr!("If the output should include full transactions or not."),
      vec![common_type::bool],
    )
      .into(),
    (
      cstr!("Node"),
      cstr!("The ethereum node block variable to use."),
      vec![NODE_VAR],
    )
      .into(),
  ];
}

impl Block for EthBlock {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.Block-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.Block")
  }

  fn name(&mut self) -> &str {
    "Eth.Block"
  }

  fn inputTypes(&mut self) -> &Vec<Type> {
    &INPUT_TYPES
  }

  fn outputTypes(&mut self) -> &Vec<Type> {
    if self.full {
      &OUTPUT_TYPES2
    } else {
      &OUTPUT_TYPES
    }
  }

  fn parameters(&mut self) -> Option<&Parameters> {
    Some(&PARAMETERS)
  }

  fn setParam(&mut self, index: i32, value: &Var) {
    match index {
      0 => self.full = value.try_into().unwrap(),
      1 => self.node_param.set_param(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.full.into(),
      1 => self.node_param.get_param(),
      _ => unreachable!(),
    }
  }

  fn requiredVariables(&mut self) -> Option<&ExposedTypes> {
    self.requiring.clear();
    let exp_info = ExposedInfo {
      exposedType: NODE_TYPE,
      name: self.node_param.get_name(),
      help: cstr!("The required ethereum node to use as gateway.").into(),
      ..ExposedInfo::default()
    };
    self.requiring.push(exp_info);
    Some(&self.requiring)
  }

  fn warmup(&mut self, context: &Context) -> Result<(), &str> {
    self.node_param.warmup(context);
    Ok(())
  }

  fn cleanup(&mut self) {
    self.node_param.cleanup();
    self.node = None;
  }

  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    Ok(activate_blocking(self, context, input))
  }
}

impl BlockingBlock for EthBlock {
  fn activate_blocking(&mut self, _: &Context, input: &Var) -> Result<Var, &str> {
    if self.node.is_none() {
      self.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        self.node_param.get(),
        &NODE_TYPE,
      )?);
    }
    let block_number: u64 = input.try_into()?;
    let node = Var::get_mut_from_clone(&self.node)?;
    let (scheduler, eth) = (&mut node.scheduler, &mut node.web3.eth());
    if self.full {
      let fut = eth.block_with_txs(BlockId::Number(block_number.into()));
      scheduler.block_on(async {
        let timed_fut = timeout(self.timeout, fut);
        let fut_res = timed_fut.await;
        if let Ok(res) = fut_res {
          match res {
            Ok(value) => {
              if let Some(value) = value {
                bytes_to_var_no_opt!(self.output, value.parent_hash, 32, "parent_hash");
                bytes_to_var_no_opt!(self.output, value.uncles_hash, 32, "uncles_hash");
                bytes_to_var_no_opt!(self.output, value.author, 20, "author");
                bytes_to_var_no_opt!(self.output, value.state_root, 32, "state_root");
                bytes_to_var_no_opt!(
                  self.output,
                  value.transactions_root,
                  32,
                  "transactions_root"
                );
                bytes_to_var_no_opt!(self.output, value.receipts_root, 32, "receipts_root");
                bytes_to_var_no_opt!(self.output, value.gas_used, 32, "gas_used");
                bytes_to_var_no_opt!(self.output, value.gas_limit, 32, "gas_limit");
                bytes_to_var_no_opt!(self.output, value.timestamp, 32, "timestamp");
                bytes_to_var_no_opt!(self.output, value.difficulty, 32, "difficulty");
                let tvar = self.output.get_mut_fast_static(cstr!("transactions"));
                let mut transactions: Seq = tvar.try_into()?;
                for value in value.transactions {
                  let mut tab = Table::new();
                  // mandatory keys
                  tab.insert_fast_static(cstr!("input"), value.input.0.as_slice().into());
                  let bytes: [u8; 32] = value.gas.into();
                  let bytes = &bytes[..];
                  tab.insert_fast_static(cstr!("gas"), bytes.into());
                  let bytes: [u8; 32] = value.gas_price.into();
                  let bytes = &bytes[..];
                  tab.insert_fast_static(cstr!("gas_price"), bytes.into());
                  let bytes: [u8; 32] = value.value.into();
                  let bytes = &bytes[..];
                  tab.insert_fast_static(cstr!("value"), bytes.into());
                  let bytes: [u8; 32] = value.nonce.into();
                  let bytes = &bytes[..];
                  tab.insert_fast_static(cstr!("nonce"), bytes.into());
                  // optional keys
                  bytes_to_var!(tab, value.from, 20, "from");
                  bytes_to_var!(tab, value.to, 20, "to");
                  bytes_to_var!(tab, value.v, 8, "v");
                  bytes_to_var!(tab, value.r, 32, "r");
                  bytes_to_var!(tab, value.s, 32, "s");
                  transactions.push(tab.as_ref().into());
                }
                *tvar = transactions.as_ref().into();
                Ok(self.output.as_ref().into())
              } else {
                Err("Block not found")
              }
            }
            Err(e) => {
              cblog!("Block error: {}", e);
              Err("Block request failed")
            }
          }
        } else {
          Err("Block request timedout")
        }
      })
    } else {
      let fut = eth.block(BlockId::Number(block_number.into()));
      scheduler.block_on(async {
        let timed_fut = timeout(self.timeout, fut);
        let fut_res = timed_fut.await;
        if let Ok(res) = fut_res {
          match res {
            Ok(value) => {
              if let Some(value) = value {
                bytes_to_var_no_opt!(self.output, value.parent_hash, 32, "parent_hash");
                bytes_to_var_no_opt!(self.output, value.uncles_hash, 32, "uncles_hash");
                bytes_to_var_no_opt!(self.output, value.author, 20, "author");
                bytes_to_var_no_opt!(self.output, value.state_root, 32, "state_root");
                bytes_to_var_no_opt!(
                  self.output,
                  value.transactions_root,
                  32,
                  "transactions_root"
                );
                bytes_to_var_no_opt!(self.output, value.receipts_root, 32, "receipts_root");
                bytes_to_var_no_opt!(self.output, value.gas_used, 32, "gas_used");
                bytes_to_var_no_opt!(self.output, value.gas_limit, 32, "gas_limit");
                bytes_to_var_no_opt!(self.output, value.timestamp, 32, "timestamp");
                bytes_to_var_no_opt!(self.output, value.difficulty, 32, "difficulty");
                let tvar = self.output.get_mut_fast_static(cstr!("transactions"));
                let mut transactions: Seq = tvar.try_into()?;
                for transaction in value.transactions {
                  let bytes: [u8; 32] = transaction.into();
                  let bytes = &bytes[..];
                  transactions.push(bytes.into());
                }
                *tvar = transactions.as_ref().into();
                Ok(self.output.as_ref().into())
              } else {
                Err("Block not found")
              }
            }
            Err(e) => {
              cblog!("Block error: {}", e);
              Err("Block request failed")
            }
          }
        } else {
          Err("Block request timedout")
        }
      })
    }
  }
}
