fn main() {
    embed_resource::compile("setup.rc", embed_resource::NONE).manifest_optional().unwrap();
}
