use crate::blocks::NODE_VAR;
use chainblocks::block::Block;
use chainblocks::cbstr;
use chainblocks::core::activate_blocking;
use chainblocks::core::BlockingBlock;
use chainblocks::cstr;
use chainblocks::types::common_type;
use chainblocks::types::Context;
use chainblocks::types::Parameters;
use chainblocks::types::RawString;
use chainblocks::types::Type;
use chainblocks::types::Var;

#[derive(Default)]
pub struct Deploy {}

static TRANSACTION_TABLE_TYPES: &'static [Type] =
    &[common_type::bytes, common_type::int, common_type::bytes];
const TRANSACTION_TABLE_KEYS: &[RawString] = &[
    cbstr!("transaction_hash"),
    cbstr!("transaction_index"),
    cbstr!("contract_address"),
];
static TRANSACTION_TABLE_TYPE: Type = Type::table(TRANSACTION_TABLE_KEYS, TRANSACTION_TABLE_TYPES);

lazy_static! {
    static ref INPUT_TYPES: Vec<Type> = vec![common_type::none];
    static ref OUTPUT_TYPES: Vec<Type> = vec![TRANSACTION_TABLE_TYPE];
    static ref PARAMETERS: Parameters = vec![
        (
            cstr!("Method"),
            cstr!("The method of the contract to call."),
            vec![common_type::string],
        )
            .into(),
        (
            cstr!("SecretKey"),
            cstr!("The sender secret key. Using a file is safer as it will not be kept in memory."),
            vec![
                common_type::path,
                common_type::path_var,
                common_type::string,
                common_type::string_var,
            ],
        )
            .into(),
        (
            cstr!("Confirmations"),
            cstr!("The amount of confirmations required."),
            vec![common_type::int],
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

impl Block for Deploy {
    fn hash() -> u32 {
        compile_time_crc32::crc32!("Eth.Deploy-rust-0x20200101")
    }

    fn registerName() -> &'static str {
        cstr!("Eth.Deploy")
    }
    fn name(&mut self) -> &str {
        "Eth.Deploy"
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
    fn setParam(&mut self, _index: i32, _value: &Var) {}
    fn getParam(&mut self, _index: i32) -> Var {
        Var::default()
    }
    fn activate(&mut self, context: &Context, input: &Var) -> Result<Var, &str> {
        Ok(activate_blocking(self, context, input))
    }
}

impl BlockingBlock for Deploy {
    fn activate_blocking(&mut self, _: &Context, _: &Var) -> Result<Var, &str> {
        todo!()
    }
}
