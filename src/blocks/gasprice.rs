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
use std::rc::Rc;
use std::str;
use std::time::Duration;
use tokio::time::timeout;
use web3::types::U256;

pub struct GasPrice {
  node_param: ParamVar,
  node: Option<Rc<Option<NodeData>>>,
  output: ClonedVar,
  timeout: Duration,
  requiring: ExposedTypes,
}

lazy_static! {
  static ref IN_TYPES: Vec<Type> = vec![common_type::none];
  static ref OUT_TYPES: Vec<Type> = vec![common_type::bytes];
  static ref PARAMETERS: Parameters = vec![(
    cstr!("Node"),
    cstr!("The ethereum node block variable to use."),
    vec![NODE_VAR],
  )
    .into(),];
}

impl Default for GasPrice {
  fn default() -> Self {
    GasPrice {
      node_param: ParamVar::new(Var::context_variable(cstr!("default.Eth"))),
      node: None,
      output: ClonedVar(Var::default()),
      timeout: get_timeout(),
      requiring: Vec::new(),
    }
  }
}

impl Block for GasPrice {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.GasPrice-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.GasPrice")
  }

  fn name(&mut self) -> &str {
    "Eth.GasPrice"
  }

  fn inputTypes(&mut self) -> &Vec<Type> {
    &IN_TYPES
  }
  fn outputTypes(&mut self) -> &Vec<Type> {
    &OUT_TYPES
  }

  fn parameters(&mut self) -> Option<&Parameters> {
    Some(&PARAMETERS)
  }
  fn setParam(&mut self, index: i32, value: &Var) {
    match index {
      0 => self.node_param.set_param(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.node_param.get_param(),
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

impl BlockingBlock for GasPrice {
  fn activate_blocking(&mut self, _: &Context, _input: &Var) -> Result<Var, &str> {
    if self.node.is_none() {
      self.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        self.node_param.get(),
        &NODE_TYPE,
      )?);
    }
    let node = Var::get_mut_from_clone(&self.node)?;
    let (scheduler, eth) = (&mut node.scheduler, &mut node.web3.eth());
    let res = scheduler
      .block_on(async {
        let fut = eth.gas_price();
        let timed_fut = timeout(self.timeout, fut);
        timed_fut.await
      })
      .or_else(|_| Err("Timed out"))?;
    match res {
      Ok(value) => {
        let u: U256 = value.into();
        let ubits: [u8; 32] = u.into();
        let sbits = &ubits[..];
        self.output = sbits.into();
        Ok(self.output.0)
      }
      Err(e) => {
        cblog!("GasPrice error: {}", e);
        Err("Failed to fetch current gas price")
      }
    }
  }
}
