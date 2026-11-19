fn main() {
    // Icône de l'exécutable (logo Hearth).
    embed_resource::compile("hearth.rc", embed_resource::NONE).manifest_optional().unwrap();
}
