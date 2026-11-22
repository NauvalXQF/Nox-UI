const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_MANIFEST: u16 = 24;

fn res_entry(out: &mut Vec<u8>, typ: u16, name: u16, flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&typ.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0x0409u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn ico_to_res(ico: &[u8], group: u16, manifest: Option<&[u8]>) -> Vec<u8> {
    let u16_at = |o: usize| u16::from_le_bytes([ico[o], ico[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes([ico[o], ico[o + 1], ico[o + 2], ico[o + 3]]);
    assert!(ico.len() > 6 && u16_at(2) == 1, "assets/nox.ico bukan berkas ikon");
    let count = u16_at(4) as usize;
    let mut res = Vec::new();
    res_entry(&mut res, 0, 0, 0, &[]);
    let mut dir = Vec::new();
    dir.extend_from_slice(&0u16.to_le_bytes());
    dir.extend_from_slice(&1u16.to_le_bytes());
    dir.extend_from_slice(&(count as u16).to_le_bytes());
    for i in 0..count {
        let e = 6 + 16 * i;
        let (size, offset) = (u32_at(e + 8) as usize, u32_at(e + 12) as usize);
        let id = (i + 1) as u16;
        res_entry(&mut res, RT_ICON, id, 0x1010, &ico[offset..offset + size]);
        let planes = u16_at(e + 4).max(1);
        let bits = if u16_at(e + 6) == 0 { 32 } else { u16_at(e + 6) };
        dir.extend_from_slice(&ico[e..e + 4]);
        dir.extend_from_slice(&planes.to_le_bytes());
        dir.extend_from_slice(&bits.to_le_bytes());
        dir.extend_from_slice(&(size as u32).to_le_bytes());
        dir.extend_from_slice(&id.to_le_bytes());
    }
    res_entry(&mut res, RT_GROUP_ICON, group, 0x1030, &dir);
    if let Some(m) = manifest {
        res_entry(&mut res, RT_MANIFEST, 1, 0x1030, m);
    }
    res
}

fn embed_icon() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest_dir.join("../../assets/nox.ico");
    let manifest = manifest_dir.join("nox.manifest");
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed={}", manifest.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let data = std::fs::read(&ico).expect("assets/nox.ico tidak terbaca");
    let man_bytes = std::fs::read(&manifest).ok();
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let res_path = out_dir.join("nox_icon.res");
    std::fs::write(&res_path, ico_to_res(&data, 1, man_bytes.as_deref())).expect("gagal menulis nox_icon.res");

    if env == "msvc" {
        if man_bytes.is_some() {
            println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        }
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
    } else if env == "gnu" {
        let obj_path = out_dir.join("nox_icon.o");
        let windres = ["windres", "x86_64-w64-mingw32-windres", r"C:\msys64\mingw64\bin\windres.exe"]
            .into_iter()
            .find_map(|tool| {
                std::process::Command::new(tool)
                    .args(["-J", "res", "-O", "coff", "-i"])
                    .arg(&res_path)
                    .arg("-o")
                    .arg(&obj_path)
                    .status()
                    .ok()
                    .filter(|s| s.success())
            });
        if windres.is_some() {
            println!("cargo:rustc-link-arg-bins={}", obj_path.display());
        } else {
            println!("cargo:warning=windres tidak ditemukan; ikon & manifest MSIX dilewati pada target gnu");
        }
    }
}
