// use chainblocks::cblog;
// use chainblocks::core::log;
use chainblocks::types::Seq;
use chainblocks::types::{ClonedVar, Var};
use ethabi::token::Token;
use json::JsonValue;
use regex::Regex;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::str;
use web3::contract::tokens::Detokenize;
use web3::contract::tokens::Tokenizable;
use web3::types::H160;
use web3::types::H256;
use web3::types::{Address, U256};

pub struct MyTokens(pub Vec<Token>);

impl Detokenize for MyTokens {
    fn from_tokens(
        tokens: std::vec::Vec<ethabi::token::Token>,
    ) -> std::result::Result<Self, web3::contract::Error> {
        // for token in &tokens {
        //   cblog!("output token: {}", token);
        // }
        Ok(MyTokens(tokens))
    }
}

fn var_seq_to_token<'a>(input: &Var, matching: &str, input_type: &str) -> Result<Token, &'a str> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"\[\d*?\]").unwrap();
    }
    if matching == "bytes" {
        // for now we pack in uint256, 32 bytes chunks
        // in order to pack into something defined
        // this should be documented
        let mut bytes = Vec::<u8>::new();
        let slice: Seq = input.try_into().unwrap();
        for v in slice.iter() {
            let vslice: &[u8] = v.as_ref().try_into()?;
            let u: U256 = vslice.into();
            let ua: [u8; 32] = u.into();
            bytes.extend_from_slice(&ua[..]);
        }
        Ok(Token::Bytes(bytes))
    } else {
        let matches: Vec<_> = RE.find_iter(&input_type[matching.len()..]).collect();
        if let Some(last) = matches.last() {
            let s = last.as_str();
            let mut sub_tokens = Vec::<Token>::new();
            let slice: Seq = input.try_into().unwrap();
            for v in slice.iter() {
                let token = var_to_token(&v, &input_type[..matching.len() + last.start()])?;
                sub_tokens.push(token);
            }
            if s.len() == 2 {
                // []
                Ok(Token::Array(sub_tokens))
            } else {
                // [N]
                Ok(Token::FixedArray(sub_tokens))
            }
        } else {
            Err("Var input was a sequence but the ABI did not expect an array")
        }
    }
}

pub fn var_to_token<'a>(input: &Var, input_type: &str) -> Result<Token, &'a str> {
    // TODO/WIP handle more types
    if input.is_seq() {
        if input_type.starts_with("uint256") {
            var_seq_to_token(input, "uint256", input_type)
        } else if input_type.starts_with("address") {
            var_seq_to_token(input, "address", input_type)
        } else if input_type.starts_with("bytes") {
            var_seq_to_token(input, "bytes", input_type)
        } else {
            Err("Not implemented input seq into token")
        }
    } else {
        if let Ok(value) = String::try_from(input) {
            // remove any possible 0x prefix
            let svalue: &str = {
                if value.starts_with("0x") {
                    &value[2..]
                } else {
                    value.as_str()
                }
            };

            if input_type == "address" {
                let address: Address = svalue
                    .parse()
                    .or_else(|_| Err("Failed to parse an input address"))?;
                Ok(address.into_token())
            } else if input_type == "uint256" {
                let uvalue: U256 = svalue
                    .parse()
                    .or_else(|_| Err("Failed to parse an input string to big int"))?;
                Ok(uvalue.into_token())
            } else {
                Err("Not implemented input into token under String")
            }
        } else if let Ok(value) = bool::try_from(input) {
            if input_type != "bool" {
                Err("Found an invalid Bool type argument")
            } else {
                Ok(value.into_token())
            }
        } else if let Ok(value) = i64::try_from(input) {
            if input_type != "uint256" {
                Err("Found an invalid Int type argument")
            } else {
                let u: U256 = value.into();
                Ok(u.into_token())
            }
        } else if let Ok(value) = input.try_into() {
            // into &[u8]
            if input_type == "uint256" {
                let slice: &[u8] = value;
                let u: U256 = slice.into();
                Ok(u.into_token())
            } else if input_type == "bytes" {
                let slice: &[u8] = value;
                Ok(Token::Bytes(slice.into()))
            } else {
                Err("Found an invalid Bytes type argument under &[u8]")
            }
        } else {
            Err("Not implemented input into token")
        }
    }
}

pub fn var_to_tokens<'a>(input: &Var, input_types: &Vec<String>) -> Result<Vec<Token>, &'a str> {
    let args: &[Var] = input.try_into().unwrap_or(&[]);
    if args.len() != input_types.len() {
        return Err("Invalid number of inputs, please check the abi again");
    }

    let mut tokens = Vec::<Token>::new();
    for i in 0..args.len() {
        let arg = &args[i];
        let input_type = &input_types[i];
        tokens.push(var_to_token(arg, input_type)?);
    }

    // for token in &tokens {
    //   cblog!("input token: {}", token);
    // }

    Ok(tokens)
}

pub fn tokens_to_var<'a>(tokens: MyTokens, output: &mut ClonedVar) -> Result<(), &'a str> {
    let mut vars = Vec::<ClonedVar>::new();
    for token in tokens.0 {
        match token {
            Token::Uint(value) => {
                let u: U256 = value.into();
                let ubits: [u8; 32] = u.into();
                let sbits = &ubits[..];
                vars.push(sbits.into());
            }
            Token::Address(value) => {
                // this is just a H160
                let h: H160 = value.into();
                let ubits: [u8; 20] = h.into();
                let sbits = &ubits[..];
                vars.push(sbits.into());
            }
            Token::Array(value) => {
                let mut subv = ClonedVar(Var::default());
                tokens_to_var(MyTokens(value), &mut subv)?;
                vars.push(subv.into());
            }
            Token::FixedArray(value) => {
                let mut subv = ClonedVar(Var::default());
                tokens_to_var(MyTokens(value), &mut subv)?;
                vars.push(subv.into());
            }
            Token::Bytes(value) => {
                let slice = value.as_slice();
                vars.push(slice.into());
            }
            Token::FixedBytes(value) => {
                let slice = value.as_slice();
                vars.push(slice.into());
            }
            _ => return Err("Found a not implemented token to var"),
        }
    }
    *output = vars.as_slice().into();
    Ok(())
}

pub fn gather_inputs<'a>(method: &str, json_abi: &JsonValue) -> Result<Vec<String>, &'a str> {
    if !json_abi.is_array() {
        Err("Invalid JSON, array expected")
    } else {
        let mut res = Vec::<String>::new();
        let mut found = false;
        for val in json_abi.members() {
            let val_method = &val["name"];
            if let Some(name) = val_method.as_str() {
                if name == method {
                    found = true;
                    let inputs = &val["inputs"];
                    if inputs.is_array() {
                        for input in inputs.members() {
                            let ty = &input["type"];
                            if let Some(tyname) = ty.as_str() {
                                res.push(tyname.into());
                            }
                        }
                    }
                }
            }
        }
        if !found {
            Err("Method not found in contract")
        } else {
            Ok(res)
        }
    }
}

pub fn hash_event<'a>(event: &str, json_abi: &JsonValue) -> Result<H256, &'a str> {
    if !json_abi.is_array() {
        Err("Invalid JSON, array expected")
    } else {
        let mut s: String = event.into();
        let mut found = false;
        s.push('(');
        for val in json_abi.members() {
            let val_method = &val["name"];
            if let Some(name) = val_method.as_str() {
                if name == event {
                    found = true;
                    let inputs = &val["inputs"];
                    if inputs.is_array() {
                        for (i, input) in inputs.members().enumerate() {
                            let ty = &input["type"];
                            if i != 0 {
                                s.push(',');
                            }
                            if let Some(tyname) = ty.as_str() {
                                s.push_str(tyname);
                            } else {
                                return Err("Failed to get type's string");
                            }
                        }
                    }
                }
            }
        }
        if !found {
            Err("Event not found in contract")
        } else {
            s.push(')');
            let hash = web3::signing::keccak256(s.as_bytes());
            Ok(hash.into())
        }
    }
}
