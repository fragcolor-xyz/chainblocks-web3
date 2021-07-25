use crate::blocks::NodeData;
use crate::blocks::Transport;
use crate::blocks::NODE_TYPE;
use chainblocks::block::Block;
use chainblocks::core::do_blocking;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::Context;
use chainblocks::types::ExposedInfo;
use chainblocks::types::ExposedTypes;
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::Type;
use chainblocks::types::Types;
use chainblocks::types::Var;
use std::convert::TryInto;
use std::ffi::CString;
use std::rc::Rc;
use std::str;
use tokio::runtime;

pub struct Eth {
  exposing: ExposedTypes,
  node_url: CString,
  node: Rc<Option<NodeData>>,
  instance: ParamVar,
  instance_name: CString,
  init_done: bool,
}

lazy_static! {
  static ref INOUT_TYPES: Vec<Type> = vec![common_type::any];
  static ref PARAMETERS: Parameters = vec![
    (
      cstr!("Url"),
      cstr!("The http/https/ws/wss address to the ethereum node."),
      vec![common_type::string],
    )
      .into(),
    (
      cstr!("Name"),
      cstr!("The name of this Eth instance to expose."),
      vec![common_type::string],
    )
      .into(),
  ];
}

impl Default for Eth {
  // add code here
  fn default() -> Self {
    Eth {
      exposing: Vec::new(),
      node_url: CString::new("https://cloudflare-eth.com").unwrap(),
      node: Rc::new(None),
      instance: ParamVar::new(().into()),
      instance_name: CString::new("default.Eth").unwrap(),
      init_done: false,
    }
  }
}

impl Block for Eth {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth")
  }

  fn name(&mut self) -> &str {
    "Eth"
  }
  fn inputTypes(&mut self) -> &Types {
    &INOUT_TYPES
  }
  fn outputTypes(&mut self) -> &Types {
    &INOUT_TYPES
  }

  fn parameters(&mut self) -> Option<&Parameters> {
    Some(&PARAMETERS)
  }

  fn setParam(&mut self, index: i32, value: &Var) {
    match index {
      0 => self.node_url = value.try_into().unwrap_or(CString::new("").unwrap()),
      1 => self.instance_name = value.try_into().unwrap_or(CString::new("").unwrap()),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.node_url.as_ref().into(),
      1 => self.instance_name.as_ref().into(),
      _ => Var::default(),
    }
  }

  fn exposedVariables(&mut self) -> Option<&ExposedTypes> {
    self.exposing.clear();
    let exp_info = ExposedInfo {
      exposedType: NODE_TYPE,
      name: self.instance_name.as_ptr(),
      help: cstr!("The exposed ethereum node instance to use as gateway.").into(),
      ..ExposedInfo::default()
    };
    self.exposing.push(exp_info);
    Some(&self.exposing)
  }

  fn warmup(&mut self, context: &Context) -> Result<(), &str> {
    self.instance.set_name(self.instance_name.to_str().unwrap());
    self.instance.warmup(context);
    Ok(())
  }

  fn cleanup(&mut self) {
    self.instance.cleanup();
    self.init_done = false;
    self.node = Rc::new(None);
  }

  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    if !self.init_done {
      Ok(do_blocking(context, || -> Result<Var, &str> {
        let uri = self.node_url.to_str().or_else(|_| Err("Invalid string"))?;
        let scheduler = runtime::Builder::new_current_thread()
          .enable_all()
          .build()
          .expect("Failed to create tokio runtime!");

        if let Ok(transport) = scheduler
          .block_on(web3::transports::WebSocket::new(uri))
          .or_else(|_| Err("WebSocket creation failed"))
        {
          let et: Transport = web3::transports::Either::Right(transport);
          let web3 = web3::Web3::new(et);

          let node_data = NodeData {
            web3: web3,
            scheduler: scheduler,
          };

          // commit what we created into the shared data
          self.node = Rc::new(Some(node_data));
          self.instance.set(Var::new_object(&self.node, &NODE_TYPE));

          return Ok(*input);
        }
        if let Ok(transport) = web3::transports::Http::new(uri) {
          let et: Transport = web3::transports::Either::Left(transport);
          let web3 = web3::Web3::new(et);

          let node_data = NodeData {
            web3: web3,
            scheduler: scheduler,
          };

          // commit what we created into the shared data
          self.node = Rc::new(Some(node_data));
          self.instance.set(Var::new_object(&self.node, &NODE_TYPE));
          self.init_done = true;
          Ok(*input)
        } else {
          Err("Failed to open remote node")
        }
      }))
    } else {
      Ok(*input)
    }
  }
}
