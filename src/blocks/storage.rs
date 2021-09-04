use crate::blocks::get_address;
use crate::blocks::get_timeout;
use crate::blocks::NodeData;
use crate::blocks::NODE_TYPE;
use crate::blocks::NODE_VAR;
use chainblocks::block::Block;
use chainblocks::cblog;
use chainblocks::core::activate_blocking;
use chainblocks::core::log;
use chainblocks::core::BlockingBlock;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::ClonedVar;
use chainblocks::types::Context;
use chainblocks::types::ExposedInfo;
use chainblocks::types::ExposedTypes;
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::Type;
use chainblocks::types::Var;
use std::convert::TryInto;
use std::rc::Rc;
use std::str;
use std::time::Duration;
use tokio::time::timeout;
use web3::types::BlockNumber;
use web3::types::H256;

pub struct Storage {
  address: ParamVar,
  index: i64,
  block: Option<BlockNumber>,
  node_param: ParamVar,
  node: Option<Rc<Option<NodeData>>>,
  output: ClonedVar,
  timeout: Duration,
  requiring: ExposedTypes,
}

impl Default for Storage {
  fn default() -> Self {
    Storage {
      address: ParamVar::new(cstr!("").into()),
      index: 0,
      block: None,
      node_param: ParamVar::new(Var::context_variable(cstr!("default.Eth"))),
      node: None,
      output: ClonedVar(Var::default()),
      timeout: get_timeout(),
      requiring: Vec::new(),
    }
  }
}

lazy_static! {
  static ref INPUT_TYPES: Vec<Type> = vec![common_type::none];
  static ref OUTPUT_TYPES: Vec<Type> = vec![common_type::bytes];
  static ref PARAMETERS: Parameters = vec![
    (
      cstr!("Address"),
      cstr!("The address to read from."),
      vec![
        common_type::string,
        common_type::string_var,
        common_type::bytes,
        common_type::bytes_var
      ],
    )
      .into(),
    (
      cstr!("Index"),
      cstr!("The storage index to read."),
      vec![common_type::int],
    )
      .into(),
    (
      cstr!("Block"),
      cstr!("The optional block number to read from history."),
      vec![common_type::none, common_type::int],
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

impl Block for Storage {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.Storage-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.Storage")
  }

  fn name(&mut self) -> &str {
    "Eth.Storage"
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
      0 => self.address.set_param(value),
      1 => self.index = value.try_into().unwrap(),
      2 => {
        if value.is_none() {
          self.block = None;
        } else {
          let u: u64 = value.try_into().unwrap();
          self.block = Some(BlockNumber::Number(u.into()));
        }
      }
      3 => self.node_param.set_param(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.address.get_param(),
      1 => self.index.into(),
      2 => {
        if let Some(blk) = self.block {
          match blk {
            BlockNumber::Number(n) => n.as_u64().try_into().unwrap(),
            _ => unreachable!(),
          }
        } else {
          Var::default()
        }
      }
      3 => self.node_param.get_param(),
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
    self.address.warmup(context);
    Ok(())
  }

  fn cleanup(&mut self) {
    self.node_param.cleanup();
    self.address.cleanup();
    self.node = None;
  }

  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    Ok(activate_blocking(self, context, input))
  }
}

impl BlockingBlock for Storage {
  fn activate_blocking(&mut self, _: &Context, _: &Var) -> Result<Var, &str> {
    if self.node.is_none() {
      self.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        self.node_param.get(),
        &NODE_TYPE,
      )?);
    }
    let node = Var::get_mut_from_clone(&self.node)?;
    let (scheduler, eth) = (&mut node.scheduler, &mut node.web3.eth());
    let address = get_address(self.address.get())?;
    let fut = eth.storage(address, self.index.into(), self.block);
    scheduler.block_on(async {
      let timed_fut = timeout(self.timeout, fut);
      let fut_res = timed_fut.await;
      if let Ok(res) = fut_res {
        match res {
          Ok(value) => {
            let value: H256 = value.into();
            let value: [u8; 32] = value.into();
            let value = &value[..];
            self.output = value.into();
            Ok(self.output.0)
          }
          Err(e) => {
            cblog!("Storage error: {}", e);
            Err("Storage request failed")
          }
        }
      } else {
        Err("Storage request timedout")
      }
    })
  }
}
