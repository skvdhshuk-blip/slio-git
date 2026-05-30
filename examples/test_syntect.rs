use syntect::parsing::SyntaxSet;

fn main() {
    let ss = SyntaxSet::load_defaults_newlines();
    let by_ext = ss.find_syntax_by_extension("php");
    println!("by extension: {:?}", by_ext.map(|s| s.name.as_ref()));
    let by_token = ss.find_syntax_by_token("php");
    println!("by token: {:?}", by_token.map(|s| s.name.as_ref()));
    let by_name = ss.find_syntax_by_name("PHP");
    println!("by name: {:?}", by_name.map(|s| s.name.as_ref()));
}
