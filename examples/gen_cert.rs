use std::{fs::write, path::Path};

use rcgen::{CertifiedKey, generate_simple_self_signed};

fn main() {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["rchat.server".to_string()]).unwrap();
    let cert_path = Path::new("./certs/server_cert.der");
    let key_path = Path::new("./certs/server_key.der");
    write(cert_path, cert.der()).unwrap();
    write(key_path, signing_key.serialize_der()).unwrap();
}
