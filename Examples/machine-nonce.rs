use atomcrypte::{AtomCrypte, Config, Nonce, Rng};

fn main() {
    let nonce = Nonce::machine_nonce(Some(Rng::osrng()));
    let data = b"Hello, world!";
    let password = "super";

    let config = Config::default().with_device(atomcrypte::DeviceList::Gpu);

    let encrypted = AtomCrypteBuilder::new()
        .nonce(nonce)
        .password(password.as_str())
        .data(data)
        .config(config)
        .encrypt()
        .unwrap();

    let out = AtomCrypteBuilder::new()
        .nonce(nonce)
        .password(password.as_str())
        .data(&encrypted)
        .config(config)
        .decrypt()
        .unwrap();

    println!("Out: {}", String::from_utf8_lossy(&out))
}
