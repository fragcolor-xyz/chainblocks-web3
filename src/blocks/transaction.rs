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
use chainblocks::types::Table;
use chainblocks::types::Type;
use chainblocks::types::Var;
use std::convert::TryInto;
use std::rc::Rc;
use std::str;
use std::time::Duration;
use tokio::time::timeout;
use web3::types::TransactionId;
use web3::types::H256;

static TABLE_TYPES: &'static [Type] = &[
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
  common_type::bytes,
];
const TABLE_KEYS: &[RawString] = &[
  cbstr!("input"),
  cbstr!("gas"),
  cbstr!("gas_price"),
  cbstr!("value"),
  cbstr!("nonce"),
];
static TABLE_TYPE: Type = Type::table(TABLE_KEYS, TABLE_TYPES);

pub struct Transaction {
  node_param: ParamVar,
  node: Option<Rc<Option<NodeData>>>,
  output: Table,
  timeout: Duration,
  requiring: ExposedTypes,
}

impl Default for Transaction {
  fn default() -> Self {
    Transaction {
      node_param: ParamVar::new(Var::context_variable(cstr!("default.Eth"))),
      node: None,
      output: Table::new(),
      timeout: get_timeout(),
      requiring: Vec::new(),
    }
  }
}

lazy_static! {
  static ref INPUT_TYPES: Vec<Type> = vec![common_type::string, common_type::bytes];
  static ref OUTPUT_TYPES: Vec<Type> = vec![TABLE_TYPE];
  static ref PARAMETERS: Parameters = vec![(
    cstr!("Node"),
    cstr!("The ethereum node block variable to use."),
    vec![NODE_VAR],
  )
    .into(),];
}

impl Block for Transaction {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.Transaction-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.Transaction")
  }

  fn name(&mut self) -> &str {
    "Eth.Transaction"
  }

  fn inputTypes(&mut self) -> &Vec<Type> {
    &INPUT_TYPES
  }

  fn outputTypes(&mut self) -> &Vec<Type> {
    &OUTPUT_TYPES
  }

  fn parameters(&mut self) -> Option<&Parameters> {
    Some(&PARAMETERS)
  }

  fn setParam(&mut self, index: i32, value: &Var) {
    match index {
      0 => self.node_param.setParam(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.node_param.getParam(),
      _ => unreachable!(),
    }
  }

  fn requiredVariables(&mut self) -> Option<&ExposedTypes> {
    self.requiring.clear();
    let exp_info = ExposedInfo {
      exposedType: NODE_TYPE,
      name: self.node_param.getName(),
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
    self.output = Table::new();
  }

  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    Ok(activate_blocking(self, context, input))
  }
}

macro_rules! bytes_to_var {
  ($output:expr, $val:expr, $bits:expr, $name:expr) => {{
    if let Some(val) = $val {
      let bytes: [u8; $bits] = val.into();
      let bytes = &bytes[..];
      $output.insert_fast_static(cstr!($name), bytes.into());
    }
  }};
}

impl BlockingBlock for Transaction {
  fn activate_blocking(&mut self, _: &Context, input: &Var) -> Result<Var, &str> {
    if self.node.is_none() {
      self.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        self.node_param.get(),
        &NODE_TYPE,
      )?);
    }
    let node = Var::get_mut_from_clone(&self.node)?;
    let (scheduler, eth) = (&mut node.scheduler, &mut node.web3.eth());
    let hash: &[u8] = input.try_into()?;
    let mut tx_hash = H256::zero();
    tx_hash.assign_from_slice(hash);
    let fut = eth.transaction(TransactionId::Hash(tx_hash));
    scheduler.block_on(async {
      let timed_fut = timeout(self.timeout, fut);
      let fut_res = timed_fut.await;
      if let Ok(res) = fut_res {
        match res {
          Ok(value) => {
            if let Some(value) = value {
              // mandatory keys
              self
                .output
                .insert_fast_static(cstr!("input"), value.input.0.as_slice().into());
              let bytes: [u8; 32] = value.gas.into();
              let bytes = &bytes[..];
              self.output.insert_fast_static(cstr!("gas"), bytes.into());
              let bytes: [u8; 32] = value.gas_price.into();
              let bytes = &bytes[..];
              self
                .output
                .insert_fast_static(cstr!("gas_price"), bytes.into());
              let bytes: [u8; 32] = value.value.into();
              let bytes = &bytes[..];
              self.output.insert_fast_static(cstr!("value"), bytes.into());
              let bytes: [u8; 32] = value.nonce.into();
              let bytes = &bytes[..];
              self.output.insert_fast_static(cstr!("nonce"), bytes.into());
              // optional keys
              bytes_to_var!(self.output, value.from, 20, "from");
              bytes_to_var!(self.output, value.to, 20, "to");
              bytes_to_var!(self.output, value.v, 8, "v");
              bytes_to_var!(self.output, value.r, 32, "r");
              bytes_to_var!(self.output, value.s, 32, "s");
              bytes_to_var!(self.output, value.block_hash, 32, "block_hash");
              if let Some(block_number) = value.block_number {
                self
                  .output
                  .insert_fast_static(cstr!("block_number"), block_number.as_u64().try_into()?);
              }
              Ok((&self.output).into())
            } else {
              Err("Transaction not found")
            }
          }
          Err(e) => {
            cblog!("Transaction error: {}", e);
            Err("Transaction request failed")
          }
        }
      } else {
        Err("Transaction request timedout")
      }
    })
  }
}
