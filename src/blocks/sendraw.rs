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

pub struct SendRaw {
  node_param: ParamVar,
  node: Option<Rc<Option<NodeData>>>,
  timeout: Duration,
  requiring: ExposedTypes,
  output_hash: Option<[u8; 32]>,
}

lazy_static! {
  static ref IN_TYPES: Vec<Type> = vec![common_type::bytes];
  static ref OUT_TYPES: Vec<Type> = vec![common_type::bytes];
  static ref PARAMETERS: Parameters = vec![(
    cstr!("Node"),
    cstr!("The ethereum node block variable to use."),
    vec![NODE_VAR],
  )
    .into(),];
}

impl Default for SendRaw {
  fn default() -> Self {
    SendRaw {
      node_param: ParamVar::new(Var::context_variable(cstr!("default.Eth"))),
      node: None,
      timeout: get_timeout(),
      requiring: Vec::new(),
      output_hash: None,
    }
  }
}

impl Block for SendRaw {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.SendRaw-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.SendRaw")
  }

  fn name(&mut self) -> &str {
    "Eth.SendRaw"
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
  }
  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    Ok(activate_blocking(self, context, input))
  }
}

impl BlockingBlock for SendRaw {
  fn activate_blocking(&mut self, _: &Context, input: &Var) -> Result<Var, &str> {
    let bytes: &[u8] = input.try_into()?;
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
        // TODO, use https://docs.rs/web3/0.15.0/web3/confirm/fn.send_raw_transaction_with_confirmation.html
        let fut = eth.send_raw_transaction(bytes.into());
        let timed_fut = timeout(self.timeout, fut);
        timed_fut.await
      })
      .or_else(|_| Err("Timed out"))?;
    match res {
      Ok(value) => {
        self.output_hash = Some(value.into());
        let slice = &self.output_hash.unwrap()[..];
        Ok(slice.into())
      }
      Err(e) => {
        cblog!("SendRaw error: {}", e);
        Err("Failed to send raw transaction")
      }
    }
  }
}
