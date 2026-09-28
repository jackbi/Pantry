fn main() {
    // 图标也是构建输入：`icons/icon.icns` 会被嵌进二进制，dev 模式的 Dock 图标用的就是它。
    // tauri-build 只声明了 src/ 与 tauri.conf.json，换图标时 cargo 会认为"没有变化"，
    // 结果重启 dev 还是旧图标——所以这里补上依赖，让改图标能触发重新编译。
    println!("cargo:rerun-if-changed=icons");

    tauri_build::build()
}
