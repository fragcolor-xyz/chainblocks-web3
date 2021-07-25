use crate::blocks::tokens::hash_event;
use crate::blocks::ContractUser;
use crate::blocks::{ContractData, EthData, NodeData};
use crate::blocks::{CONTRACT_TYPE, CONTRACT_VAR, NODE_TYPE};
use chainblocks::block::Block;
// use chainblocks::cblog;
use chainblocks::cbstr;
use chainblocks::core::activate_blocking;
use chainblocks::core::getState;
// use chainblocks::core::log;
use chainblocks::core::BlockingBlock;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::ChainState;
use chainblocks::types::Context;
use chainblocks::types::ExposedInfo;
use chainblocks::types::ExposedTypes;
use chainblocks::types::ParamVar;
use chainblocks::types::Parameters;
use chainblocks::types::RawString;
use chainblocks::types::Seq;
use chainblocks::types::Table;
use chainblocks::types::Type;
use chainblocks::types::Types;
use chainblocks::types::Var;
use futures::stream::StreamExt;
use std::convert::TryInto;
use std::ffi::CString;
use std::str;
use std::time::Duration;
use tokio::time::timeout;
use web3::transports::Either;
use web3::types::FilterBuilder;
use web3::types::H256;

pub struct WaitEvent {
  cu: ContractUser,
  event_hash: H256,
  sub: Option<web3::api::SubscriptionStream<web3::transports::WebSocket, web3::types::Log>>,
  output: Table,
  scratch: Seq,
}

static LOGS_TABLE_TYPES: &'static [Type] = &[common_type::bytes, common_type::bytezs];
const LOGS_TABLE_KEYS: &[RawString] = &[cbstr!("data"), cbstr!("topics")];
static LOGS_TABLE_TYPE: Type = Type::table(LOGS_TABLE_KEYS, LOGS_TABLE_TYPES);

lazy_static! {
  static ref INPUT_TYPES: Vec<Type> = vec![common_type::none];
  static ref OUTPUT_TYPES: Vec<Type> = vec![LOGS_TABLE_TYPE];
  static ref PARAMETERS: Parameters = vec![
    (
      cstr!("Event"),
      cstr!("The event of the contract to listen for."),
      vec![common_type::string],
    )
      .into(),
    (
      cstr!("Contract"),
      cstr!("The contract instance we operate."),
      vec![CONTRACT_VAR],
    )
      .into(),
  ];
}

impl Default for WaitEvent {
  fn default() -> Self {
    WaitEvent {
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
      event_hash: H256::zero(),
      sub: None,
      output: Table::new(),
      scratch: Seq::new(),
    }
  }
}

impl Block for WaitEvent {
  fn hash() -> u32 {
    compile_time_crc32::crc32!("Eth.WaitEvent-rust-0x20200101")
  }

  fn registerName() -> &'static str {
    cstr!("Eth.WaitEvent")
  }

  fn name(&mut self) -> &str {
    "Eth.WaitEvent"
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
      0 => self.cu.data.method = value.try_into().unwrap_or(CString::new("").unwrap()),
      1 => self.cu.instance.set_param(value),
      _ => unreachable!(),
    }
  }

  fn getParam(&mut self, index: i32) -> Var {
    match index {
      0 => self.cu.data.method.as_ref().into(),
      1 => self.cu.instance.get_param(),
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
    if !self.cu.instance.is_variable() {
      Err("Contract instance is empty or not valid")
    } else {
      self.cu.instance.warmup(context);
      Ok(())
    }
  }

  fn cleanup(&mut self) {
    self.cu.instance.cleanup();
    self.cu.node = None;
    self.cu.data.contract = None;
    self.output = Table::new();
  }

  fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
    if self.cu.data.contract.is_none() {
      self.cu.data.contract = Some(Var::from_object_as_clone::<Option<ContractData>>(
        self.cu.instance.get(),
        &CONTRACT_TYPE,
      )?);
      let contract = Var::get_mut_from_clone(&self.cu.data.contract)?;
      let method = self
        .cu
        .data
        .method
        .to_str()
        .or_else(|_| Err("Invalid string"))?;

      // cache the event hash
      self.event_hash = hash_event(method, &contract.json_abi)?;

      // also populate node data here
      self.cu.node = Some(Var::from_object_as_clone::<Option<NodeData>>(
        contract.node,
        &NODE_TYPE,
      )?);
    }
    Ok(activate_blocking(self, context, input))
  }
}

impl BlockingBlock for WaitEvent {
  fn activate_blocking(
    &mut self,
    context: &Context,
    _input: &Var,
  ) -> std::result::Result<Var, &str> {
    // subscribe if needed
    if self.sub.is_none() {
      let node = Var::get_mut_from_clone(&self.cu.node)?;
      let (scheduler, transport) = (&mut node.scheduler, node.web3.transport());

      let contract = Var::get_mut_from_clone(&self.cu.data.contract)?;
      let event_hash = self.event_hash;
      self.sub = Some(scheduler.block_on(async {
        let filter = FilterBuilder::default()
          .address(vec![contract.contract.address()])
          .topics(Some(vec![event_hash]), None, None, None)
          .build();
        match transport {
          Either::Right(ws) => {
            let web3 = web3::Web3::new(ws.clone());
            let sub = web3
              .eth_subscribe()
              .subscribe_logs(filter)
              .await
              .or_else(|_| Err("Failed to subscribe to topic"))?;
            Ok(sub)
          }
          _ => Err("WaitEvents needs a WebSocket backed Eth node block"),
        }
      })?);
    }

    // wait for an event
    if let Some(sub) = &mut self.sub {
      let node = Var::get_mut_from_clone(&self.cu.node)?;
      node.scheduler.block_on(work_async(
        sub,
        context,
        &mut self.output,
        &mut self.scratch,
      ))?;
      Ok((&self.output).into())
    } else {
      Err("Subscription was empty")
    }
  }
}

async fn work_async<'a>(
  sub: &mut web3::api::SubscriptionStream<web3::transports::WebSocket, web3::types::Log>,
  context: &Context,
  output: &mut Table,
  scratch: &mut Seq,
) -> Result<(), &'a str> {
  loop {
    let fut = sub.next();
    let timed_fut = timeout(Duration::from_secs(1), fut);
    let fut_res = timed_fut.await;
    // handle chain state asap
    if getState(context) == ChainState::Stop {
      return Ok(());
    }
    // continue if timedout
    if let Ok(opt_data) = fut_res {
      if let Some(data) = opt_data {
        if let Ok(logs) = data {
          // mandatory stuff
          output.insert_fast_static(cstr!("data"), logs.data.0.as_slice().into());

          scratch.clear();
          for topic in logs.topics {
            scratch.push(topic.as_bytes().into());
          }
          output.insert_fast_static(cstr!("topics"), scratch.as_ref().into());

          // optional stuff
          if let Some(block_hash) = logs.block_hash {
            output.insert_fast_static(cstr!("block_hash"), block_hash.as_bytes().into());
          }
          if let Some(block_number) = logs.block_number {
            output.insert_fast_static(cstr!("block_number"), block_number.as_u64().try_into()?);
          }
          if let Some(transaction_hash) = logs.transaction_hash {
            output.insert_fast_static(
              cstr!("transaction_hash"),
              transaction_hash.as_bytes().into(),
            );
          }
          if let Some(transaction_index) = logs.transaction_index {
            output.insert_fast_static(
              cstr!("transaction_index"),
              transaction_index.as_u64().try_into()?,
            );
          }
          if let Some(removed) = logs.removed {
            output.insert_fast_static(cstr!("removed"), removed.into());
          }
          return Ok(());
        } else {
          return Err("Empty logs");
        }
      } else {
        return Err("Failed to unwrap logs");
      }
    } else {
      // cblog!("event polling timedout");
      continue;
    }
  }
}
