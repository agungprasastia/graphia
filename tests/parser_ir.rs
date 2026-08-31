use graphia::model::{Language as GraphiaLanguage, NodeKind, Visibility};
use graphia::parser::parse_file;

#[test]
fn test_rust_parser_populates_ir_and_signatures() {
    let code = r#"
pub struct User {
    pub name: String,
}

#[test]
fn test_rust_parser_extracts_relationship_ir() {
    let code = "trait Render {} struct Widget; impl Render for Widget {} fn make() { Widget; Widget::new(); }";
    let pf = parse_file("widget.rs", GraphiaLanguage::Rust, code);
    assert!(pf.instantiations.iter().any(|item| item.type_name == "Widget"));
    assert!(pf.implementations.iter().any(|item| {
        item.implementing_type.contains("Widget") && item.trait_or_interface == "Render"
    }));
}

pub trait Greeter {
    fn greet(&self, target: &str) -> String;
}

pub fn login(user: User, password: &str) -> bool {
    let valid = user.name.len() > 0;
    valid
}

pub use std::collections::HashMap;
"#;
    let pf = parse_file("src/auth.rs", GraphiaLanguage::Rust, code);
    assert!(
        !pf.definitions.is_empty(),
        "definitions should not be empty"
    );
    assert!(!pf.exports.is_empty(), "exports should not be empty");
    assert!(
        !pf.type_references.is_empty(),
        "type_references should not be empty"
    );

    let login_def = pf
        .definitions
        .iter()
        .find(|d| d.name == "login")
        .expect("login definition");
    assert_eq!(login_def.kind, NodeKind::Function);
    assert_eq!(login_def.visibility, Visibility::Public);
    assert!(login_def.signature.is_some(), "signature must be extracted");
    let sig = login_def.signature.as_ref().unwrap();
    assert!(
        sig.contains("login(user:User,password:&str)->bool"),
        "signature was: {}",
        sig
    );
}

#[test]
fn test_signature_normalization_and_overloads() {
    let code1 = "fn process( a : i32 , b : String ) -> bool { true }";
    let code2 = "fn process(a: i32, b: String) -> bool { false }";
    let code3 = "fn process(a: i32) -> bool { true }";

    let pf1 = parse_file("a.rs", GraphiaLanguage::Rust, code1);
    let pf2 = parse_file("b.rs", GraphiaLanguage::Rust, code2);
    let pf3 = parse_file("c.rs", GraphiaLanguage::Rust, code3);

    let sig1 = pf1
        .definitions
        .iter()
        .find(|d| d.name == "process")
        .unwrap()
        .signature
        .clone();
    let sig2 = pf2
        .definitions
        .iter()
        .find(|d| d.name == "process")
        .unwrap()
        .signature
        .clone();
    let sig3 = pf3
        .definitions
        .iter()
        .find(|d| d.name == "process")
        .unwrap()
        .signature
        .clone();

    assert_eq!(
        sig1, sig2,
        "whitespace differences must normalize to identical signature"
    );
    assert_ne!(
        sig1, sig3,
        "different parameter counts must have different signatures"
    );
}

#[test]
fn test_typescript_parser_populates_ir_and_signatures() {
    let code = r#"
export interface Config {
    timeout: number;
}

export class Client {
    constructor(private cfg: Config) {}
    public connect(url: string): Promise<boolean> {
        return Promise.resolve(true);
    }
}

export function createClient(cfg: Config): Client {
    return new Client(cfg);
}
"#;
    let pf = parse_file("src/client.ts", GraphiaLanguage::TypeScript, code);
    assert!(
        !pf.definitions.is_empty(),
        "ts definitions should not be empty"
    );
    assert!(!pf.exports.is_empty(), "ts exports should not be empty");
    assert!(
        !pf.type_references.is_empty(),
        "ts type_references should not be empty"
    );

    let connect_def = pf
        .definitions
        .iter()
        .find(|d| d.name == "connect")
        .expect("connect definition");
    assert_eq!(connect_def.kind, NodeKind::Method);
    assert_eq!(connect_def.visibility, Visibility::Public);
    assert!(
        connect_def.signature.is_some(),
        "signature must be extracted"
    );
}

#[test]
fn test_python_parser_populates_ir_and_signatures() {
    let code = r#"
__all__ = ["AuthService"]

class AuthService:
    def __init__(self, db):
        self.db = db

    def authenticate(self, username, password):
        return True

def helper():
    pass
"#;
    let pf = parse_file("auth.py", GraphiaLanguage::Python, code);
    assert!(
        !pf.definitions.is_empty(),
        "python definitions should not be empty"
    );
    assert!(!pf.exports.is_empty(), "python exports should not be empty");

    let auth_def = pf
        .definitions
        .iter()
        .find(|d| d.name == "authenticate")
        .expect("authenticate method");
    assert_eq!(auth_def.kind, NodeKind::Method);
    assert!(auth_def.signature.is_some(), "python signature extracted");
}
