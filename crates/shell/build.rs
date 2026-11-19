fn main() {
    let config = slint_build::CompilerConfiguration::new().with_bundled_translations("lang");
    slint_build::compile_with_config("ui/app.slint", config).unwrap();
    // Icône de l'exécutable (logo Hearth).
    embed_resource::compile("hearth.rc", embed_resource::NONE).manifest_optional().unwrap();
}
