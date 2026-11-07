//! Host-side Windows EXE preflight using the very same no_std parser
//! the ZillaOS compatibility layer will use. NO execution or DLL loading.
use std::{env, fs, process};
use zillaos_kernel::compat::windows_pe::inspect_pe64;

const LIMIT: u64 = 64 * 1024 * 1024;
fn main() {
    let mut args = env::args_os().skip(1);
    let Some(path) = args.next() else {
        eprintln!("Usage: pe-inspect <windows-program.exe>");
        process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("Expected exactly one EXE path");
        process::exit(2);
    }
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("Could not open EXE: {err}");
            process::exit(1);
        }
    };
    match file.metadata() {
        Ok(meta) if meta.is_file() && meta.len() <= LIMIT => {}
        _ => {
            eprintln!("Refusing non-regular input or file larger than 64 MiB");
            process::exit(1);
        }
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("Could not read EXE: {err}");
            process::exit(1);
        }
    };
    if bytes.len() as u64 > LIMIT {
        eprintln!("Input grew beyond 64 MiB");
        process::exit(1);
    }
    match inspect_pe64(&bytes) {
        Ok(info) => {
            println!("Windows PE32+ x86-64: valid headers");
            println!("Sections: {}", info.sections);
            println!("Entry RVA: 0x{:x}", info.entry_rva);
            println!("Image base: 0x{:x}", info.image_base);
            println!("Mapped image size: {} bytes", info.image_size);
            println!("Subsystem: {}", info.subsystem);
            println!("DLL: {}", info.is_dll);
            println!("Imports: {}", info.has_imports);
            println!("Relocations: {}", info.has_relocations);
            println!("Execution: NOT AVAILABLE in ZillaOS");
            println!("Requires isolated process loader, Win32/NT API and libraries.");
        }
        Err(err) => {
            eprintln!("Not a supported PE32+ x86-64 executable: {err:?}");
            process::exit(1);
        }
    }
}
