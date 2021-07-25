use crate::blocks::get_address;
use crate::blocks::{ContractData, NodeData};
use crate::blocks::{CONTRACT_TYPE, NODE_TYPE, NODE_VAR};
use chainblocks::block::Block;
use chainblocks::cblog;
use chainblocks::core::log;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::Context;
use chainblocks::types::ExposedInfo;
use chainblocks::types::ExposedTypes;
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::Types;
use chainblocks::types::Var;
use std::convert::TryInto;
use std::ffi::CString;
use std::rc::Rc;
use std::str;
use web3::contract::Contract;

pub struct SharedContract {
  instance: ParamVar,
  instance_name: CString,
  node_param: ParamVar,
  contract_address: ParamVar,
  contract_current: Var,
  abi_json: CString,
  contract: Rc<Option<ContractData>>,
  init_done: bool,
  exposing: ExposedTypes,
  requiring: ExposedTypes,
}

lazy_static! {
  static ref INOUT_TYPES: Types = vec![common_type::any];
  static ref PARAMETERS: Parameters = vec![
    (
      cstr!("Contract"),
      cstr!("The contract address."),
      vec![
        common_type::string,
        common_type::string_var,
        common_type::bytes,
        common_type::bytes_var
      ],
    )
      .into(),
    (
      cstr!("Abi"),
      cstr!("The JSON abi of the contract"),
      vec![common_type::string],
    )
      .into(),
    (
      cstr!("Name"),
      cstr!("The instance name we want to expose."),
      vec![common_type::string],
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

impl Default for SharedContract {
  // add code here
  fn default() -> Self {
    SharedContract {
      instance: ParamVar::new(().into()),
      instance_name: CString::new("default.Eth.Contract").unwrap(),
      node_param: ParamVar::new(Var::context_variable(cstr!("default.Eth"))),
      contract_address: ParamVar::new(cstr!("").into()),
      contract_current: Var::default(),
      abi_json: CString::new("").unwrap(),
      contract: Rc::new(None),
      init_done: false,
      exposing: Vec::new(),
      requiring: Vec::new(),
    }
  }
}

impl Block for SharedContract {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.Contract-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.Contract")
  }

  fn name(&mut self) -> &str {
    "Eth.Contract"
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
      0 => self.contract_address.set_param(value),
      1 => self.abi_json = value.try_into().unwrap_or(CString::new("").unwrap()),
      2 => self.instance_name = value.try_into().unwrap_or(CString::new("").unwrap()),
      3 => self.node_param.set_param(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.contract_address.get_param(),
      1 => self.abi_json.as_ref().into(),
      2 => self.instance_name.as_ref().into(),
      3 => self.node_param.get_param(),
      _ => Var::default(),
    }
  }

  fn exposedVariables(&mut self) -> Option<&ExposedTypes> {
    self.exposing.clear();
    let exp_info = ExposedInfo {
      exposedType: CONTRACT_TYPE,
      name: self.instance_name.as_ptr(),
      help: cstr!("The exposed ethereum contract.").into(),
      ..ExposedInfo::default()
    };
    self.exposing.push(exp_info);
    Some(&self.exposing)
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

    if self.contract_address.is_variable() {
      let exp_info = ExposedInfo {
        exposedType: common_type::any,
        name: self.contract_address.get_name(),
        help: cstr!("The required contract address variable.").into(),
        ..ExposedInfo::default()
      };
      self.requiring.push(exp_info);
    }
    Some(&self.requiring)
  }

  fn warmup(&mut self, context: &Context) -> Result<(), &str> {
    self.instance.set_name(self.instance_name.to_str().unwrap());
    self.instance.warmup(context);
    self.node_param.warmup(context);
    self.contract_address.warmup(context);
    Ok(())
  }

  fn cleanup(&mut self) {
    self.instance.cleanup();
    self.node_param.cleanup();
    self.contract_address.cleanup();
    self.init_done = false;
    self.contract = Rc::new(None);
  }

  fn activate(&mut self, _context: &Context, input: &Var) -> Result<Var, &str> {
    // TODO check if contract address has changed.. it so need to re-init here!
    let vaddress = self.contract_address.get();
    if !self.init_done || self.contract_current != vaddress {
      self.contract_current = vaddress;
      let mut node_ref = Some(Var::from_object_as_clone::<Option<NodeData>>(
        self.node_param.get(),
        &NODE_TYPE,
      )?);
      let node_ptr = node_ref
        .as_mut()
        .ok_or_else(|| "Failed to unwrap node data shared pointer")?;
      let node_o = &**node_ptr;
      let node = node_o
        .as_ref()
        .ok_or_else(|| "Failed to unwrap node data, was empty")?;
      let address = get_address(vaddress)?;
      let contract = Contract::from_json(node.web3.eth(), address, self.abi_json.as_bytes())
        .or_else(|e| {
          cblog!("Contract::from_json error: {}", e);
          Err("Failed to initialize contract")
        })?;
      self.contract = Rc::new(Some(ContractData {
        contract: contract,
        json_abi: json::parse(
          self
            .abi_json
            .to_str()
            .or_else(|_| Err("Invalid abi string"))?,
        )
        .or_else(|_| Err("Failed to parse contract's json abi"))?,
        node: self.node_param.get(),
      }));
      self
        .instance
        .set(Var::new_object(&self.contract, &CONTRACT_TYPE));
      self.init_done = true;
    }
    Ok(*input)
  }
}
