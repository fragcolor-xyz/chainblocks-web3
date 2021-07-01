#[macro_use]
extern crate ctor;
extern crate log;
#[macro_use]
extern crate lazy_static;
extern crate compile_time_crc32;
extern crate regex;

#[cfg(test)]
mod tests {
  extern crate tokio;
  extern crate web3;
  use web3::contract::Contract;
  use web3::contract::Options;
  use web3::types::{Address, U256};

  #[tokio::test]
  async fn it_works() {
    assert_eq!(2 + 2, 4);
    let transport = web3::transports::Http::new("https://cloudflare-eth.com").unwrap();
    let web3 = web3::Web3::new(transport);
    let accounts = web3.eth().accounts().await.unwrap();
    println!("Accounts: {:?}", accounts);
    let account = "EB29D5C25AF0A35D2d0b85D3798788f81A0BF7Ae".parse().unwrap();
    let balance = web3.eth().balance(account, None).await.unwrap();
    let k = web3::types::U256::from_dec_str("1000000000000000000").unwrap();
    let (ediv, emod) = balance.div_mod(k);
    println!("Balance of {:?}: {} {}", account, ediv, emod);

    let from_coin: Address = "6b175474e89094c44da98b954eedeac495271d0f".parse().unwrap();
    let to_coin: Address = "EeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".parse().unwrap();
    let contract = Contract::from_json(
      web3.eth(),
      "C586BeF4a0992C495Cf22e1aeEE4E446CECDee0E".parse().unwrap(),
      include_bytes!("../tests/onesplit.json"),
    )
    .unwrap();

    let query_res = contract.query(
      "getExpectedReturn",
      (
        from_coin,
        to_coin,
        U256::from(1000),
        U256::from(100),
        U256::from(0),
      ),
      None,
      Options::default(),
      None,
    );
    let _actual_res: (U256, Vec<U256>) = query_res.await.unwrap();
  }

  use web3::futures::{future, StreamExt};

  #[tokio::test]
  async fn pubsub() -> web3::Result {
    let uri = concat!("wss://mainnet.infura.io/ws/v3/", include_str!("infura.key"));
    let ws = web3::transports::WebSocket::new(uri).await?;
    let web3 = web3::Web3::new(ws.clone());
    let mut sub = web3.eth_subscribe().subscribe_new_heads().await?;

    println!("Got subscription id: {:?}", sub.id());

    (&mut sub)
      .take(5)
      .for_each(|x| {
        println!("Got: {:?}", x);
        future::ready(())
      })
      .await;

    sub.unsubscribe();

    Ok(())
  }
}

#[cfg(not(test))]
mod blocks {
  mod contract;
  mod tokens;
  mod currentblock;
  mod estimategas;
  mod eth;
  mod gasprice;
  mod read;
  mod read_batch;
  mod sendraw;
  mod storage;
  mod transaction;
  mod unlock;
  mod waitevent;
  mod write;

  extern crate chainblocks;
  extern crate futures;
  extern crate json;
  extern crate secp256k1;
  extern crate tokio;
  extern crate web3;
  extern crate zeroize;

  use chainblocks::core::init;
  use chainblocks::core::registerBlock;
  use chainblocks::types::ExposedTypes;
  use chainblocks::types::ParamVar;
  use chainblocks::types::Type;
  use chainblocks::types::Var;
  use json::JsonValue;
  use std::convert::TryInto;
  use std::env;
  use std::ffi::CString;
  use std::rc::Rc;
  use std::time::Duration;
  use tokio::runtime::Runtime;
  use web3::contract::Contract;
  use web3::types::Address;

  use currentblock::CurrentBlock;
  use estimategas::EstimateGas;
  use contract::SharedContract;
  use eth::Eth;
  use gasprice::GasPrice;
  use read::Read;
  use read_batch::ReadBatch;
  use sendraw::SendRaw;
  use storage::Storage;
  use transaction::Transaction;
  use unlock::Unlock;
  use waitevent::WaitEvent;
  use write::Write;

  type Transport =
    web3::transports::either::Either<web3::transports::Http, web3::transports::WebSocket>;

  struct NodeData {
    web3: web3::Web3<Transport>,
    scheduler: Runtime,
  }

  static NODE_TYPE: Type = Type::object(1936289387, 1702127694);
  static NODE_TYPE_VEC: &'static [Type] = &[NODE_TYPE];
  static NODE_VAR: Type = Type::context_variable(NODE_TYPE_VEC);

  struct ContractData {
    contract: Contract<Transport>,
    json_abi: JsonValue,
    node: Var,
  }

  static CONTRACT_TYPE: Type = Type::object(1936289387, 1702127683);
  static CONTRACT_TYPE_VEC: &'static [Type] = &[CONTRACT_TYPE];
  static CONTRACT_VAR: Type = Type::context_variable(CONTRACT_TYPE_VEC);

  struct EthData {
    contract: Option<Rc<Option<ContractData>>>,
    method: CString,
    from: Option<CString>,
    input_types: Vec<String>,
  }

  struct ContractUser {
    instance: ParamVar,
    from: ParamVar,
    data: EthData,
    node: Option<Rc<Option<NodeData>>>,
    requiring: ExposedTypes,
  }

  pub fn get_address<'a>(v: Var) -> Result<Address, &'a str> {
    let address: Address = {
      let saddress: Result<&str, &str> = v.as_ref().try_into();
      let baddress: Result<&[u8], &str> = v.as_ref().try_into();
      if let Ok(s) = saddress {
        if s.starts_with("0x") {
          let subs: &str = &s[2..];
          subs
            .parse()
            .or_else(|_| Err("Failed to parse Contract address"))?
        } else {
          s.parse()
            .or_else(|_| Err("Failed to parse Contract address"))?
        }
      } else if let Ok(b) = baddress {
        let a20: [u8; 20] = b
          .try_into()
          .or_else(|_| Err("Invalid bytes for contract address"))?;
        a20.into()
      } else {
        return Err("Invalid address type");
      }
    };
    Ok(address)
  }

  pub fn get_timeout() -> Duration {
    let key = "WEB3_TIMEOUT";
    match env::var(key) {
      Ok(val) => {
        if let Ok(secs) = val.parse::<u64>() {
          Duration::from_secs(secs)
        } else {
          Duration::from_secs(30)
        }
      }
      Err(_) => Duration::from_secs(30),
    }
  }

  #[ctor]
  fn register_blocks() {
    env_logger::init();
    init();
    registerBlock::<Eth>();
    registerBlock::<CurrentBlock>();
    registerBlock::<EstimateGas>();
    registerBlock::<SharedContract>();
    registerBlock::<Read>();
    registerBlock::<Write>();
    registerBlock::<WaitEvent>();
    registerBlock::<Storage>();
    registerBlock::<Transaction>();
    registerBlock::<Unlock>();
    registerBlock::<GasPrice>();
    registerBlock::<ReadBatch>();
    registerBlock::<SendRaw>();
  }
}
