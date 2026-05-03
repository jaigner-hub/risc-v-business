use std::{env, fs, path::Path};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let tests_dir = Path::new("tests/riscv-tests");

    let mut code = String::from(
        "fn run_riscv_test(path: &std::path::Path) {\n\
            let bytes = std::fs::read(path).expect(\"read test ELF\");\n\
            let loaded = riscv_emu::loader::load_elf(&bytes).expect(\"load ELF\");\n\
            let tohost = loaded.tohost_addr.expect(\"no tohost symbol\");\n\
            let mut cpu = riscv_emu::cpu::Cpu::new(loaded.bus, loaded.entry, false);\n\
            for _ in 0..100_000_000u64 {\n\
                match cpu.step() {\n\
                    Ok(()) => {}\n\
                    Err(e) => panic!(\"cpu error: {e}\"),\n\
                }\n\
                if let Ok(v) = cpu.bus.load(tohost, 8) {\n\
                    if v != 0 {\n\
                        assert_eq!(v, 1, \"test failed: tohost={v:#x} file={}\", path.display());\n\
                        return;\n\
                    }\n\
                }\n\
            }\n\
            panic!(\"test timed out: {}\", path.display());\n\
        }\n\n"
    );

    let mut found = false;
    if let Ok(entries) = fs::read_dir(tests_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            let is_test_elf = name.starts_with("rv64ui-p-")
                || name.starts_with("rv64um-p-")
                || name.starts_with("rv64ua-p-")
                || name.starts_with("rv64mi-p-");
            if !is_test_elf || name.ends_with(".dump") || name.ends_with(".o") || name == ".gitkeep" {
                continue;
            }
            found = true;
            let fn_name = name.replace('-', "_");
            let abs_path = path.canonicalize().unwrap();
            code.push_str(&format!(
                "#[test]\nfn {fn_name}() {{\n    \
                    run_riscv_test(std::path::Path::new({:?}));\n}}\n\n",
                abs_path.to_str().unwrap()
            ));
        }
    }

    if !found {
        code.push_str(
            "#[test]\nfn _no_riscv_tests_vendored() {\n    \
                panic!(\"No riscv-tests ELFs found in tests/riscv-tests/. \
                Run scripts/fetch-riscv-tests.sh first.\");\n}\n"
        );
    }

    fs::write(Path::new(&out_dir).join("riscv_tests_generated.rs"), code).unwrap();
    println!("cargo:rerun-if-changed=tests/riscv-tests");
}
