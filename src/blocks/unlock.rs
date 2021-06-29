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
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::Type;
use chainblocks::types::Var;
use std::convert::TryInto;
use std::ffi::CString;
use std::rc::Rc;
use std::str;
use std::time::Duration;
use tokio::time::timeout;

pub struct Unlock {
  node_param: ParamVar,
  node: Option<Rc<Option<NodeData>>>,
  password_param: ParamVar,
  address: CString,
  timeout: Duration,
}

impl Default for Unlock {
  fn default() -> Self {
    Unlock {
      node_param: ParamVar::new(Var::context_variable(cstr!("default.Eth"))),
      node: None,
      password_param: ParamVar::new(cstr!("").into()),
      address: CString::new("").unwrap(),
      timeout: get_timeout(),
    }
  }
}

lazy_static! {
  static ref INOUT_TYPES: Vec<Type> = vec![common_type::any];
  static ref PARAMETERS: Parameters = vec![
    (
      cstr!("Address"),
      cstr!("The node's address to unlock for a single transaction."),
      vec![common_type::string],
    )
      .into(),
    (
      cstr!("Password"),
      cstr!("The password to unlock the account."),
      vec![common_type::string, common_type::string_var,],
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

impl Block for Unlock {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.Unlock-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.Unlock")
  }

  fn name(&mut self) -> &str {
    "Eth.Unlock"
  }
  fn inputTypes(&mut self) -> &Vec<Type> {
    &INOUT_TYPES
  }
  fn outputTypes(&mut self) -> &Vec<Type> {
    &INOUT_TYPES
  }
  fn parameters(&mut self) -> Option<&Parameters> {
    Some(&PARAMETERS)
  }
  fn setParam(&mut self, index: i32, value: &Var) {
    match index {
      0 => self.address = value.try_into().unwrap(),
      1 => self.password_param.setParam(value),
      2 => self.node_param.setParam(value),
      _ => unreachable!(),
    }
  }
  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.address.as_ref().into(),
      1 => self.password_param.getParam(),
      2 => self.node_param.getParam(),
      _ => unreachable!(),
    }
  }
  fn warmup(&mut self, context: &Context) -> Result<(), &str> {
    self.node_param.warmup(context);
    self.password_param.warmup(context);
    Ok(())
  }
  fn cleanup(&mut self) {
    self.node_param.cleanup();
    self.password_param.cleanup();
    self.node = None;
  }
  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    Ok(activate_blocking(self, context, input))
  }
}

impl BlockingBlock for Unlock {
  fn activate_blocking(&mut self, _: &Context, input: &Var) -> Result<Var, &str> {
    if self.node.is_none() {
      self.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        self.node_param.get(),
        &NODE_TYPE,
      )?);
    }
    let node = Var::get_mut_from_clone(&self.node)?;
    let (scheduler, personal) = (&mut node.scheduler, node.web3.personal());
    let address = {
      if let Ok(s) = self.address.to_str() {
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
    }?;
    let passwd = self.password_param.get().as_ref().try_into()?;
    let res = scheduler
      .block_on(async {
        let fut = personal.unlock_account(address, passwd, None);
        let timed_fut = timeout(self.timeout, fut);
        timed_fut.await
      })
      .or_else(|_| Err("Timed out"))?;
    match res {
      Ok(success) => {
        if success {
          Ok(*input)
        } else {
          Err("Failed to unlock account")
        }
      }
      Err(e) => {
        cblog!("Account unlock error: {}", e);
        Err("Failed to unlock account")
      }
    }
  }
}
