use syntect::dumps::dump_to_file;
use syntect::highlighting::ThemeSet;

fn main() {
    let ts = ThemeSet::load_from_folder("assets").unwrap();
    dump_to_file(&ts, "assets/ansi.bin").unwrap();
}
