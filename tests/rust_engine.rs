use antigravity_region_fix::{
    engine::{self, Kind, State},
    format::{executable_images, Arch},
};
use std::fs;

fn hex(s: &str) -> Vec<u8> {
    s.split_ascii_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}
fn cli_x64() -> Vec<u8> {
    hex("48 85 c0 0f 84 0d 02 00 00 80 78 08 00 0f 85 03 02 00 00 e8 68 f1 fd ff 48 89 84 24 80 00 00 00 48 89 5c 24 50 48 89 4c 24 70")
}
fn cli_arm() -> Vec<u8> {
    hex("e1 18 00 b5 c0 0d 00 b4 01 20 40 39 81 0d 00 37 be 94 ff 97 e0 4b 00 f9 e1 33 00 f9 e2 43 00 f9")
}
fn elf(code: &[u8], machine: u16) -> Vec<u8> {
    let mut b = vec![0; 512 + code.len()];
    b[..6].copy_from_slice(b"\x7fELF\x02\x01");
    b[18..20].copy_from_slice(&machine.to_le_bytes());
    b[32..40].copy_from_slice(&64u64.to_le_bytes());
    b[54..56].copy_from_slice(&56u16.to_le_bytes());
    b[56..58].copy_from_slice(&1u16.to_le_bytes());
    b[64..68].copy_from_slice(&1u32.to_le_bytes());
    b[68..72].copy_from_slice(&1u32.to_le_bytes());
    b[72..80].copy_from_slice(&512u64.to_le_bytes());
    b[96..104].copy_from_slice(&(code.len() as u64).to_le_bytes());
    b[512..].copy_from_slice(code);
    b
}
fn pe(code: &[u8], machine: u16) -> Vec<u8> {
    let mut b = vec![0; 1024];
    b[..2].copy_from_slice(b"MZ");
    b[60..64].copy_from_slice(&128u32.to_le_bytes());
    b[128..132].copy_from_slice(b"PE\0\0");
    b[132..134].copy_from_slice(&machine.to_le_bytes());
    b[134..136].copy_from_slice(&1u16.to_le_bytes());
    b[168..172].copy_from_slice(&(code.len() as u32).to_le_bytes());
    b[172..176].copy_from_slice(&512u32.to_le_bytes());
    b[188..192].copy_from_slice(&0x60000020u32.to_le_bytes());
    b[512..512 + code.len()].copy_from_slice(code);
    b
}
fn macho(code: &[u8], cpu: u32) -> Vec<u8> {
    let mut b = vec![0; 1024];
    b[..4].copy_from_slice(b"\xcf\xfa\xed\xfe");
    b[4..8].copy_from_slice(&cpu.to_le_bytes());
    b[16..20].copy_from_slice(&1u32.to_le_bytes());
    b[20..24].copy_from_slice(&152u32.to_le_bytes());
    b[32..36].copy_from_slice(&0x19u32.to_le_bytes());
    b[36..40].copy_from_slice(&152u32.to_le_bytes());
    b[96..100].copy_from_slice(&1u32.to_le_bytes());
    b[104..110].copy_from_slice(b"__text");
    b[120..126].copy_from_slice(b"__TEXT");
    b[144..152].copy_from_slice(&(code.len() as u64).to_le_bytes());
    b[152..156].copy_from_slice(&512u32.to_le_bytes());
    b[168..172].copy_from_slice(&0x80000400u32.to_le_bytes());
    b[512..512 + code.len()].copy_from_slice(code);
    b
}
fn fat(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = vec![0; 256 + a.len() + b.len()];
    out[..4].copy_from_slice(b"\xca\xfe\xba\xbe");
    out[4..8].copy_from_slice(&2u32.to_be_bytes());
    for (i, (slice, offset, cpu)) in [(a, 256, 0x01000007u32), (b, 256 + a.len(), 0x0100000cu32)]
        .into_iter()
        .enumerate()
    {
        let p = 8 + i * 20;
        out[p..p + 4].copy_from_slice(&cpu.to_be_bytes());
        out[p + 8..p + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[p + 12..p + 16].copy_from_slice(&(slice.len() as u32).to_be_bytes());
        out[offset..offset + slice.len()].copy_from_slice(slice);
    }
    out
}
#[test]
fn all_supported_formats_and_architectures() {
    for (code, machine, cpu) in [(cli_x64(), 0x3e, 0x01000007), (cli_arm(), 0xb7, 0x0100000c)] {
        for data in [
            elf(&code, machine),
            pe(&code, if machine == 0x3e { 0x8664 } else { 0xaa64 }),
            macho(&code, cpu),
        ] {
            assert_eq!(
                engine::inspect(&data, Kind::Cli).unwrap().state,
                State::Original
            );
            let patched = engine::patched_bytes(&data, Kind::Cli).unwrap();
            assert_eq!(
                engine::inspect(&patched, Kind::Cli).unwrap().state,
                State::Patched
            );
        }
    }
}
#[test]
fn universal_macho_patches_both_slices() {
    let data = fat(
        &macho(&cli_x64(), 0x01000007),
        &macho(&cli_arm(), 0x0100000c),
    );
    let images = executable_images(&data).unwrap();
    assert_eq!(images[0].arch, Arch::X64);
    assert_eq!(images[1].arch, Arch::Arm64);
    let patched = engine::patched_bytes(&data, Kind::Cli).unwrap();
    assert_eq!(
        engine::inspect(&patched, Kind::Cli).unwrap().state,
        State::Patched
    );
}
#[test]
fn universal_mixed_state_is_rejected() {
    let a = engine::patched_bytes(&macho(&cli_x64(), 0x01000007), Kind::Cli).unwrap();
    assert!(engine::inspect(&fat(&a, &macho(&cli_arm(), 0x0100000c)), Kind::Cli).is_err());
}
#[test]
fn duplicate_and_mixed_signatures_are_rejected() {
    let mut code = cli_x64();
    code.extend(cli_x64());
    assert!(engine::inspect(&elf(&code, 0x3e), Kind::Cli).is_err());
    let patched = engine::patched_bytes(&elf(&cli_x64(), 0x3e), Kind::Cli).unwrap();
    let mut code = cli_x64();
    code.extend(&patched[512..]);
    assert!(engine::inspect(&elf(&code, 0x3e), Kind::Cli).is_err());
}
#[test]
fn data_section_matches_are_ignored() {
    let mut data = pe(&cli_x64(), 0x8664);
    data[768..768 + cli_x64().len()].copy_from_slice(&cli_x64());
    assert_eq!(
        engine::inspect(&data, Kind::Cli).unwrap().state,
        State::Original
    );
}
#[test]
fn wrong_arm_registers_are_rejected() {
    let mut code = cli_arm();
    code[0] = 0xe2;
    assert!(engine::inspect(&elf(&code, 0xb7), Kind::Cli).is_err());
}
#[test]
fn architecture_mismatch_is_rejected() {
    assert!(engine::inspect(&elf(&cli_x64(), 0xb7), Kind::Cli).is_err());
}
#[test]
fn hub_variants() {
    for (code, machine) in [
        (
            hex("80 78 08 00 74 3e 48 8b 4c 24 78 48 89 48 40 48 8b 8c 24 80 00 00 00 48 89 48 48"),
            0x3e,
        ),
        (hex("80 78 08 00 74 3e 48 8b 4c 24 78 48 89 48 60"), 0x3e),
        (
            hex("03 20 40 39 c3 01 00 36 e3 03 40 f9 e4 13 48 a9 03 10 06 a9"),
            0xb7,
        ),
        (hex("03 20 40 39 a3 01 00 36 e3 13 48 a9 03 10 06 a9"), 0xb7),
    ] {
        let data = elf(&code, machine);
        assert_eq!(
            engine::inspect(&engine::patched_bytes(&data, Kind::App).unwrap(), Kind::App)
                .unwrap()
                .state,
            State::Patched
        );
    }
}
#[test]
fn ide_requires_unique_gate() {
    let original = b"before;resetIsTierGCPTos(),this._productService.isGoogleInternal;after";
    assert!(engine::patched_bytes(
        &[original.as_slice(), original.as_slice()].concat(),
        Kind::Ide
    )
    .is_err());
    let patched = engine::patched_bytes(original, Kind::Ide).unwrap();
    assert_eq!(patched, b"before;resetIsTierGCPTos(),true;after");
}
#[test]
fn truncated_executables_never_panic() {
    let data = elf(&cli_x64(), 0x3e);
    for length in 0..data.len() {
        assert!(engine::inspect(&data[..length], Kind::Cli).is_err());
    }
}
#[test]
fn ide_patch_restore_and_backup_verification() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app/resources/app/out/main.js");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = b"resetIsTierGCPTos(),this.product.isGoogleInternal";
    fs::write(&path, original).unwrap();
    engine::change(&path, Kind::Ide, false).unwrap();
    assert!(!engine::change(&path, Kind::Ide, false).unwrap());
    engine::change(&path, Kind::Ide, true).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
}
#[test]
fn tampered_backup_is_not_restored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app/resources/app/out/main.js");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"resetIsTierGCPTos(),this.product.isGoogleInternal").unwrap();
    engine::change(&path, Kind::Ide, false).unwrap();
    let patched = fs::read(&path).unwrap();
    fs::write(
        engine::sidecar(&path, ".agybak"),
        b"different;resetIsTierGCPTos(),this.product.isGoogleInternal",
    )
    .unwrap();
    assert!(engine::change(&path, Kind::Ide, true).is_err());
    assert_eq!(fs::read(&path).unwrap(), patched);
}
#[test]
fn failed_record_write_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app/resources/app/out/main.js");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = b"resetIsTierGCPTos(),this.product.isGoogleInternal";
    fs::write(&path, original).unwrap();
    fs::create_dir(engine::sidecar(&path, ".pagy.json")).unwrap();
    assert!(engine::change(&path, Kind::Ide, false).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
}
#[test]
fn unknown_file_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unknown");
    fs::write(&path, b"unknown").unwrap();
    assert!(engine::change(&path, Kind::Cli, false).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"unknown");
    assert!(!engine::sidecar(&path, ".agybak").exists());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn pagy_detects_update_and_lock_contention() {
    use fs2::FileExt;
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(if cfg!(windows) { "agy.exe" } else { "agy" });
    let original = elf(&cli_x64(), 0x3e);
    fs::write(&path, &original).unwrap();
    let state = dir.path().join("state");
    fs::create_dir(&state).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_pagy"))
            .arg("--help")
            .env("PAGY_AGY", &path)
            .env("PAGY_STATE_DIR", &state)
            .output()
            .unwrap()
    };
    let lock = fs::File::create(state.join("patch.lock")).unwrap();
    FileExt::lock_exclusive(&lock).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read(&path).unwrap(), original);
    drop(lock);
    let first = run();
    assert!(!first.status.success());
    assert_eq!(
        engine::inspect(&fs::read(&path).unwrap(), Kind::Cli)
            .unwrap()
            .state,
        State::Patched
    );
    assert!(String::from_utf8_lossy(&first.stderr).contains("patch applied"));
    assert!(!String::from_utf8_lossy(&run().stderr).contains("patch applied"));
    let mut updated = original;
    updated[256] = 42;
    fs::write(&path, &updated).unwrap();
    let second = run();
    assert!(String::from_utf8_lossy(&second.stderr).contains("patch applied"));
    assert_eq!(
        fs::read(engine::sidecar(&path, ".agybak")).unwrap(),
        updated
    );
}

#[test]
fn native_wrapper_preserves_arguments_streams_and_exit_status() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("probe.rs");
    let executable = dir
        .path()
        .join(if cfg!(windows) { "probe.exe" } else { "probe" });
    let bytes = if cfg!(target_arch = "aarch64") {
        cli_arm()
    } else {
        cli_x64()
    };
    let assembly = format!(
        ".byte {}",
        bytes
            .iter()
            .map(|b| format!("0x{b:02x}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let code = r#"
use std::{io::Read,ffi::OsStr};
#[inline(never)]
fn gate() -> ! { unsafe { std::arch::asm!("ASSEMBLY", options(noreturn)) } }
fn encode(value: &OsStr) -> String {
    #[cfg(unix)] let bytes = { use std::os::unix::ffi::OsStrExt; value.as_bytes().to_vec() };
    #[cfg(windows)] let bytes = value.to_string_lossy().as_bytes().to_vec();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn main() {
    std::hint::black_box(gate as fn() -> !);
    for arg in std::env::args_os().skip(1) { println!("{}", encode(&arg)); }
    println!("cwd:{}",encode(std::env::current_dir().unwrap().as_os_str()));
    println!("env:{}",std::env::var("PAGY_TEST_MARKER").unwrap());
    let mut input=String::new(); std::io::stdin().read_to_string(&mut input).unwrap();
    println!("stdin:{}",encode(OsStr::new(&input))); eprintln!("probe-stderr"); std::process::exit(19);
}
"#.replace("ASSEMBLY",&assembly);
    fs::write(&source, code).unwrap();
    let compile = Command::new("rustc")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let args: Vec<std::ffi::OsString> = vec![
        "--help",
        "--",
        "",
        "a b",
        "русский",
        "\"quoted\"",
        "$HOME",
        "first\nsecond",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    #[cfg(unix)]
    let args = {
        use std::os::unix::ffi::OsStringExt;
        let mut args = args;
        args.push(std::ffi::OsString::from_vec(vec![0xff, 0xfe]));
        args
    };
    let run = |program: &std::path::Path| {
        let mut child = Command::new(program)
            .args(&args)
            .env("PAGY_AGY", &executable)
            .env("PAGY_STATE_DIR", dir.path().join("state"))
            .env("PAGY_TEST_MARKER", "value")
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"stdin-value")
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let first = run(std::path::Path::new(env!("CARGO_BIN_EXE_pagy")));
    assert_eq!(
        first.status.code(),
        Some(19),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let wrapped = run(std::path::Path::new(env!("CARGO_BIN_EXE_pagy")));
    let direct = run(&executable);
    assert_eq!(wrapped.status.code(), direct.status.code());
    assert_eq!(wrapped.stdout, direct.stdout);
    assert_eq!(wrapped.stderr, direct.stderr);
}

#[test]
fn native_installer_is_reversible_and_rejects_unrelated_commands() {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin with spaces");
    let command = env!("CARGO_BIN_EXE_antigravity-region-fix");
    let run = |args: &[&std::ffi::OsStr]| {
        Command::new(command)
            .args(args)
            .env("PAGY_STATE_DIR", dir.path().join("state"))
            .output()
            .unwrap()
    };
    let installed = run(&["install".as_ref(), "--bin-dir".as_ref(), bin.as_os_str()]);
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    assert!(bin
        .join(format!("pagy{}", std::env::consts::EXE_SUFFIX))
        .is_file());
    assert!(
        run(&["install".as_ref(), "--bin-dir".as_ref(), bin.as_os_str()])
            .status
            .success()
    );
    let removed = run(&["uninstall".as_ref()]);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let path = bin.join(format!("pagy{}", std::env::consts::EXE_SUFFIX));
    fs::write(&path, b"unrelated command").unwrap();
    assert!(
        !run(&["install".as_ref(), "--bin-dir".as_ref(), bin.as_os_str()])
            .status
            .success()
    );
    assert_eq!(fs::read(path).unwrap(), b"unrelated command");
}
