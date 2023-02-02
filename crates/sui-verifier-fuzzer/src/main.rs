use arbitrary::{Arbitrary, Unstructured};
use move_binary_format::file_format::CompiledModule;
use std::io::Read;
use std::{collections::BTreeMap, env};
use sui_verifier::{
    entry_points_verifier, global_storage_access_verifier, id_leak_verifier,
    one_time_witness_verifier, private_generics, struct_with_key_verifier,
    verifier as sui_bytecode_verifier,
};

fn main() {
    let mut args: Vec<String> = env::args().collect();
    match &args[..] {
        [] => unreachable!(""),
        [_] => {
            args.push("".to_owned());
            args.push("".to_owned())
        }
        [_, option] => {
            args.push(option.to_owned());
            args.push("".to_owned())
        }
        _ => (),
    };
    let debug = match args[2].as_str() {
        "--debug" | "-d" => true,
        _ => false,
    };

    let mut handle = std::io::stdin().lock();
    let mut input = Vec::new();
    let _ = handle.read_to_end(&mut input);
    let mut unstructured = Unstructured::new(&input);
    if let Ok(m) = CompiledModule::arbitrary(&mut unstructured) {
        if debug {
            dbg!(m.to_owned());
        };

        match args[1].as_str() {
            "--core-move" => {
                let _ = move_bytecode_verifier::verify_module(&m);
            }
            "--sui-move" => {
                if !debug {
                    let _ = sui_bytecode_verifier::verify_module(&m, &BTreeMap::new());
                } else {
                    dbg!("doing struct_with_key");
                    let _ = struct_with_key_verifier::verify_module(&m);
                    dbg!("doing global_storage_access");
                    let _ = global_storage_access_verifier::verify_module(&m);
                    dbg!("doing id_leak_verifier");
                    let _ = id_leak_verifier::verify_module(&m);
                    dbg!("doing private_generics");
                    let _ = private_generics::verify_module(&m);
                    dbg!("doing entry_points");
                    let _ = entry_points_verifier::verify_module(&m, &BTreeMap::new());
                    dbg!("doing one_time_witness");
                    let _ = one_time_witness_verifier::verify_module(&m, &BTreeMap::new());
                };
            }
            _ => {
                let _ = move_bytecode_verifier::verify_module(&m);
                let _ = sui_bytecode_verifier::verify_module(&m, &BTreeMap::new());
            }
        }
    }
    // Invalid...
}
