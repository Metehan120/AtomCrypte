/*! # AtomCrypte

- A high-performance, multi-layered encryption library designed for flexibility, security, and speed.

---

## 🚧 Disclaimer
This project is experimental and should not be used in production systems. It is created for academic research, cryptographic experimentation, and learning purposes. Use at your own discretion.

---

## Overview

AtomCrypte is a robust encryption library that combines multiple cryptographic techniques to provide state-of-the-art security with configurable parameters. It supports parallel processing, GPU acceleration, and modular cryptographic components, enabling both performance and advanced customization.

## Key Features

- **Salt Support**: Cryptographic salt generation using `Salt::new()` to prevent rainbow table attacks
- **Infinite Rounds**: User-defined encryption round count
- **Wrap-All Support**: Seamlessly wraps salt, nonce, version, etc. into final output
- **MAC with SHA3-512**: Strong integrity validation and quantum resistance
- **Benchmark Support**: Time encryption/decryption operations with `.benchmark()`
- **Secure Key Derivation**: Argon2 + Blake3 for password hashing
- **Dynamic S-boxes**: Based on password, nonce or both
- **Finite Field Arithmetic**: Galois Field operations similar to AES MixColumns
- **Parallel Processing**: Uses Rayon for multicore CPU support
- **GPU Acceleration**: OpenCL backend for fast encryption/decryption
- **Zeroized Memory**: Automatic clearing of sensitive data in RAM

## Cryptographic Components

AtomCrypte integrates the following primitives and concepts:

- **Argon2**: Memory-hard password hashing
- **Blake3**: Fast cryptographic hash for key derivation
- **SHA3-512**: Default MAC function with post-quantum resilience
- **Custom S-box**: Deterministic but unique per configuration
- **Galois Field**: MixColumns-like transformation layer
- **MAC Validation**: Ensures authenticity and tamper-resistance

## Configuration Options

AtomCrypte is highly configurable. Below are common customization options:

### Device Selection
```rust
pub enum DeviceList {
    Auto,
    Cpu,
    Gpu,
}
```

### S-box Generation
```rust
pub enum SboxTypes {
    PasswordBased,
    NonceBased,
    PasswordAndNonceBased,
}
```

### Galois Field Polynomial
```rust
pub enum IrreduciblePoly {
    AES,
    Custom(u8),
}
```

### Predefined Profiles
```rust
pub enum Profile {
    Secure,
    Balanced,
    Fast,
}
```

### Nonce Types
```rust
pub enum NonceData {
    TaggedNonce([u8; 32]),
    HashedNonce([u8; 32]),
    Nonce([u8; 32]),
    MachineNonce([u8; 32]),
}
```

## Usage Examples

### Basic Encryption/Decryption
```rust
use atom_crypte::{AtomCrypteBuilder, Config, Profile, Rng, Nonce};

let nonce = Nonce::nonce(Rng::osrng());
let config = Config::default();

let encrypted = AtomCrypteBuilder::new()
    .data("Hello, world!".as_bytes())
    .password("secure_password")
    .nonce(nonce)
    .config(config)
    .wrap_all(true) // Optional
    .benchmark() // Optional
    .encrypt()
    .expect("Encryption failed");

let decrypted = AtomCrypteBuilder::new()
    .data(&encrypted)
    .password("secure_password")
    .config(config)
    .wrap_all(true) // Optional
    .benchmark() // Optional
    .decrypt()
    .expect("Decryption failed");

assert_eq!(decrypted, "Hello, world!".as_bytes());
```
### How to use salt
```rust
let salt = Salt::new();
let encrypted = AtomCrypteBuilder::new()
    .data("Important secrets".as_bytes())
    .password("your_password")
    .nonce(Nonce::nonce(Rng::osrng()))
    .config(Config::default())
    .wrap_all(true) // Optional
    .salt(salt) // Optional but recommended
    .benchmark() // Optional
    .encrypt()
    .expect("Encryption failed");

// Or you can turn byte slice into Salt
```

### Custom Configuration
- 🚧 - 🚧 If you forget your configuration, you won't be able to decrypt the data. (Especially important if you changed round count, S-box type, or polynomial.)
```rust
use atom_crypte::{AtomCrypteBuilder, Config, DeviceList, SboxTypes, IrreduciblePoly};

let config = Config::default()
    .with_device(DeviceList::Gpu)
    .with_sbox(SboxTypes::PasswordAndNonceBased)
    .set_thread(4)
    .gf_poly(IrreduciblePoly::Custom(0x4d))
    .rounds(6); // 4 Rounds recommended
```

### Using Predefined Profiles
```rust
use atom_crypte::{AtomCrypteBuilder, Config, Profile};

let config = Config::from_profile(Profile::Fast);
```

### Machine-specific Encryption
```rust
use atom_crypte::{AtomCrypteBuilder, Config, Nonce};

let nonce = Nonce::machine_nonce(None); // You can generate via Machine info + Rng
let password = "your_password_here".machine_rng(false); // False means no distro lock
```

## Performance

- **CPU**: Parallelized via Rayon
- **GPU**: OpenCL enabled
- **Benchmarks**: ~100MB ≈ 1s encryption/decryption on avarage device

## Security Considerations

- Constant-time comparisons
- Memory zeroization
- Authenticated encryption with SHA3 MAC
- Configurable number of layers and rounds
- Defense-in-depth: multiple cryptographic operations layered !*/

use std::time::Instant;

use argon2::{Argon2, password_hash::SaltString};
use blake3::derive_key;
use gpu::{dynamic_shift_gpu, dynamic_unshift_gpu};
use rand::{RngCore, TryRngCore, random_range, rngs::OsRng};
use rayon::prelude::*;
use sha3::{Digest, Sha3_512};
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroize;
pub mod gpu;

static VERSION: &[u8] = b"atom-version:0x3";

/// Represents different types of nonces used in the encryption process.
/// - TaggedNonce: Nonce combined with a user-provided tag
/// - HashedNonce: Cryptographically hashed nonce for extra randomness
/// - Nonce: Standard cryptographically secure random nonce
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NonceData {
    TaggedNonce([u8; 32]),
    HashedNonce([u8; 32]),
    Nonce([u8; 32]),
    MachineNonce([u8; 32]),
} // Multiple data types for future usage

/// Represents different types of errors that can occur during encryption or decryption.
/// - This enum provides a comprehensive set of error types that can be encountered
/// - during the encryption and decryption processes. Each error variant includes a
/// - descriptive message that helps in identifying the root cause of the issue.
#[derive(Debug, Error)]
pub enum Errors {
    #[error("Decryption failed: {0}")]
    InvalidNonce(String),
    #[error("Invalid MAC: {0}")]
    InvalidMac(String),
    #[error("XOR failed: {0}")]
    InvalidXor(String),
    #[error("Thread Pool Failed: {0}")]
    ThreadPool(String),
    #[error("Argon2 failed: {0}")]
    Argon2Failed(String),
    #[error("Invalid Algorithm")]
    InvalidAlgorithm,
    #[error("Kernel Error: {0}")]
    KernelError(String),
    #[error("Build Failed: {0}")]
    BuildFailed(String),
}

/// Represents different types of devices that can be used for encryption and decryption.
#[derive(Debug, Clone, Copy)]
pub enum DeviceList {
    Auto,
    Cpu,
    Gpu,
}

/// Represents different types of sboxes that can be used for encryption and decryption.
/// # Not recommended for use in production environments.
#[derive(Debug, Clone, Copy)]
pub enum SboxTypes {
    PasswordBased,
    NonceBased,
    PasswordAndNonceBased,
}

/// Represents different types of irreducible polynomials that can be used for encryption and decryption.
#[derive(Debug, Clone, Copy)]
pub enum IrreduciblePoly {
    AES,
    Custom(u8),
}

impl IrreduciblePoly {
    fn value(&self) -> u8 {
        match self {
            IrreduciblePoly::AES => 0x1b, // x^8 + x^4 + x^3 + x + 1
            IrreduciblePoly::Custom(val) => *val,
        }
    }
}

/// Configuration for the encryption and decryption process.
/// - `device`: The device to use for encryption.
/// - `sbox`: The S-box to use for encryption.
/// - `thread_num`: The number of threads to use for encryption.
/// - `gf_poly`: The Galois field polynomial to use for encryption.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub rounds: usize,
    pub device: DeviceList,
    pub sbox: SboxTypes,
    pub thread_num: usize,
    pub gf_poly: IrreduciblePoly,
} // For feature use

/// Profile for the encryption and decryption process.
#[derive(Debug, Clone, Copy)]
pub enum Profile {
    Secure,
    Balanced,
    Fast,
}

impl Default for Config {
    /// Default configuration for the encryption and decryption process.
    fn default() -> Self {
        Self {
            device: DeviceList::Cpu,
            sbox: SboxTypes::PasswordAndNonceBased,
            thread_num: num_cpus::get(),
            gf_poly: IrreduciblePoly::AES,
            rounds: 3,
        }
    }
}

impl Config {
    /// Sets S-Box generation type for encryption and decryption.
    /// - Not recommended changing the S-Box.
    pub fn with_sbox(mut self, sbox: SboxTypes) -> Self {
        self.sbox = sbox;
        self
    }

    /// Sets the device to use for encryption and decryption.
    /// - Not recommended changing the device after initialization.
    pub fn with_device(mut self, device: DeviceList) -> Self {
        self.device = device;
        self
    }

    /// Sets the number of threads to use for encryption and decryption.
    /// - Not recommended changing the number of threads after initialization.
    pub fn set_thread(mut self, num: usize) -> Self {
        self.thread_num = num;
        self
    }

    /// Sets the Galois field polynomial to use for encryption and decryption.
    /// - Not recommended changing the Galois field polynomial after initialization.
    pub fn gf_poly(mut self, poly: IrreduciblePoly) -> Self {
        self.gf_poly = poly;
        self
    }

    /// Sets the number of rounds to use for encryption and decryption.
    /// - If you're using with version 0.2.0 data set this to 1.
    /// - Not recommended changing the number of rounds after initialization.
    pub fn rounds(mut self, num: usize) -> Self {
        if num < 1 {
            eprintln!("Round count too low. Automatically set to 1.");
            self.rounds = 1;
            return self;
        } else {
            self.rounds = num;
            self
        }
    }

    /// Create a configuration from a profile.
    pub fn from_profile(profile: Profile) -> Self {
        match profile {
            Profile::Fast => Self {
                device: DeviceList::Gpu,
                sbox: SboxTypes::PasswordBased,
                thread_num: num_cpus::get(),
                gf_poly: IrreduciblePoly::AES,
                rounds: 2,
            },

            Profile::Balanced => Self {
                device: DeviceList::Auto,
                sbox: SboxTypes::PasswordAndNonceBased,
                thread_num: num_cpus::get(),
                gf_poly: IrreduciblePoly::AES,
                rounds: 3,
            },
            Profile::Secure => Self {
                device: DeviceList::Cpu,
                sbox: SboxTypes::PasswordAndNonceBased,
                thread_num: num_cpus::get(),
                gf_poly: IrreduciblePoly::AES,
                rounds: 3,
            },
        }
    }
}

impl NonceData {
    /// Converts the nonce data into a byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        match self {
            NonceData::Nonce(n)
            | NonceData::HashedNonce(n)
            | NonceData::TaggedNonce(n)
            | NonceData::MachineNonce(n) => n,
        }
    }
    /// Converts the nonce data into a vector of bytes.
    pub fn to_vec(&self) -> Vec<u8> {
        match self {
            NonceData::Nonce(n)
            | NonceData::HashedNonce(n)
            | NonceData::TaggedNonce(n)
            | NonceData::MachineNonce(n) => n.to_vec(),
        }
    }
}

/// Converts bytes or vector of bytes into a NonceData.
pub trait AsNonce {
    fn as_nonce(&self) -> NonceData;
    fn as_nonce_safe(&self) -> Result<NonceData, String>;
}

fn slice_to_nonce(input: &[u8]) -> Result<NonceData, String> {
    if input.len() != 32 {
        Err("Nonce length must be 32 bytes".to_string())
    } else {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(input);
        Ok(NonceData::Nonce(arr))
    }
}

/// Converts the bytes into a nonce data.
impl AsNonce for [u8] {
    fn as_nonce(&self) -> NonceData {
        slice_to_nonce(self).expect("Nonce length must be 32 bytes")
    }

    fn as_nonce_safe(&self) -> Result<NonceData, String> {
        slice_to_nonce(self)
    }
}

/// Converts the bytes vector into a nonce data.
impl AsNonce for Vec<u8> {
    fn as_nonce(&self) -> NonceData {
        slice_to_nonce(self).expect("Nonce length must be 32 bytes")
    }

    fn as_nonce_safe(&self) -> Result<NonceData, String> {
        slice_to_nonce(self)
    }
}

/// Generates a random nonce using the operating system's random number generator.
pub enum Rng {
    OsRngNonce([u8; 32]),
    TaggedOsRngNonce([u8; 32]),
    ThreadRngNonce([u8; 32]),
}

impl Rng {
    /// Generates a random nonce using the machine's random number generator.
    pub fn thread_rng() -> Self {
        let mut nonce = [0u8; 32];
        rand::rng().fill_bytes(&mut nonce);
        Self::ThreadRngNonce(nonce)
    }

    /// Generates a random nonce using the operating system's random number generator.
    pub fn osrng() -> Self {
        let mut nonce = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut nonce)
            .expect("Nonce generation failed");
        Self::OsRngNonce(nonce)
    }

    /// Generates a random nonce using the operating system's random number generator, with a tag.
    pub fn tagged_osrng(tag: &[u8]) -> Self {
        let mut nonce = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut nonce)
            .expect("Nonce generation failed");

        let new_nonce: Vec<u8> = nonce
            .iter()
            .enumerate()
            .map(|(i, b)| b.wrapping_add(tag[i % tag.len()] ^ i as u8))
            .collect();

        let mut final_nonce = [0u8; 32];
        final_nonce.copy_from_slice(&new_nonce[..32]);

        Self::TaggedOsRngNonce(final_nonce)
    }

    /// Returns the RNG as a byte slice.
    pub fn as_bytes(&self) -> &[u8; 32] {
        match &self {
            Self::OsRngNonce(a) | Self::TaggedOsRngNonce(a) | Self::ThreadRngNonce(a) => a,
        }
    }

    /// Returns the RNG as a vector of bytes.
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// Generates a unique identifier based on the machine's configuration.
pub trait MachineRng {
    fn machine_rng(&self, distro_lock: bool) -> String;
}

/// Generates a unique identifier based on the machine's configuration.
/// Heads up:
/// If you're migrating from version 2.2 or used machine_rng with distribution lock enabled,
/// make sure to decrypt your data before changing or reinstalling your OS.
/// The OS distribution is a part of the key derivation process when distro_lock is set to true.
/// Failing to do so may permanently prevent access to your encrypted data.
impl MachineRng for str {
    fn machine_rng(&self, distro_lock: bool) -> String {
        let user_name = whoami::username();
        let device_name = whoami::devicename();
        let real_name = whoami::realname();

        let mut data = Vec::new();
        data.extend_from_slice(user_name.as_bytes());
        data.extend_from_slice(device_name.as_bytes());
        data.extend_from_slice(real_name.as_bytes());
        if distro_lock == true {
            let distro = whoami::distro();
            data.extend_from_slice(distro.as_bytes());
        }
        data.extend_from_slice(self.as_bytes());

        let hash = blake3::hash(&data);
        hash.to_hex().to_string()
    }
}

/// ### Builder for AtomCrypte
/// - You can encrypte & decrypte data using the builder.
pub struct AtomCrypteBuilder {
    config: Option<Config>,
    data: Option<Vec<u8>>,
    password: Option<String>,
    nonce: Option<NonceData>,
    salt: Option<Salt>,
    wrap_all: bool,
    benchmark: bool,
}

/// Generates a Unique Nonce
pub struct Nonce;

impl Nonce {
    /// # Generates a Unique Nonce via Hash
    /// - Recommended for use in most cases
    /// - Adding extra security by hashing the nonce
    pub fn hashed_nonce(osrng: Rng) -> NonceData {
        let mut nonce = *osrng.as_bytes();
        let number: u8 = rand::random_range(0..255);

        for i in 0..=number {
            let mut mix = nonce.to_vec();
            mix.push(i as u8);
            nonce = *blake3::hash(&mix).as_bytes();
        }

        NonceData::HashedNonce(nonce)
    }

    /// # Generates a Unique Nonce via Tag and Hash
    /// - Adding extra security by hashing the nonce
    /// - Adding tag to the nonce (Extra Security)
    pub fn tagged_nonce(osrng: Rng, tag: &[u8]) -> NonceData {
        let mut nonce = *osrng.as_bytes();
        let number: u8 = rand::random_range(0..255);

        for i in 0..=number {
            let mut mix = nonce.to_vec();
            mix.push(i as u8);
            nonce = *blake3::hash(&mix).as_bytes();
        }

        NonceData::TaggedNonce(*blake3::hash(&[&nonce, tag].concat()).as_bytes()) // Hash the nonce to get a 32 byte more random nonce (Extra Security)
    }

    /// Generates a Unique Nonce via Machine Info and Hash
    /// This nonce must be saved along with the encrypted data.
    /// - Adding extra security by hashing the nonce
    /// - Adding machine info to the nonce (Extra Security)
    pub fn machine_nonce(osrng: Option<Rng>) -> NonceData {
        let user_name = whoami::username();
        let device_name = whoami::devicename();
        let real_name = whoami::realname();
        let distro = whoami::distro();

        let mut all_data = Vec::new();

        all_data.extend_from_slice(user_name.as_bytes());
        all_data.extend_from_slice(device_name.as_bytes());
        all_data.extend_from_slice(real_name.as_bytes());
        all_data.extend_from_slice(distro.as_bytes());

        if let Some(rng) = osrng {
            all_data.extend_from_slice(rng.as_bytes());
        }

        let hash = blake3::hash(&all_data);

        NonceData::MachineNonce(*hash.as_bytes())
    }

    /// Generates a unique Nonce
    /// - Classic method with random bytes
    pub fn nonce(osrng: Rng) -> NonceData {
        let nonce = *osrng.as_bytes();
        let number: u8 = random_range(0..255);

        let new_nonce_vec = nonce
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let add = (osrng.as_bytes()[i % osrng.as_bytes().len()] as usize) % (i + 1);
                let add = add as u8;
                b.wrapping_add(add.wrapping_add(number))
            })
            .collect::<Vec<u8>>();

        let mut new_nonce = [0u8; 32];
        new_nonce.copy_from_slice(&new_nonce_vec[..32]);

        NonceData::Nonce(new_nonce)
    }
}

// -----------------------------------------------------

/// Generator for a new salt
/// - You can save this salt to a file or database, or you can add directly to encrypted data.
///
/// /// ⚠️ Warning:
/// If you lose this salt, decryption will fail. Keep it safe like your password.
#[derive(Debug, Copy, Clone)]
pub enum Salt {
    Salt([u8; 32]),
}

impl Salt {
    /// Generate a new salt
    /// Generates a new salt using a combination of random bytes from the thread and OS random number generators.
    /// - You have to save this salt to a file or database, or you can add directly to encrypted data.
    pub fn new() -> Self {
        let rng = *Rng::thread_rng().as_bytes();
        let mix_rng = *Rng::osrng().as_bytes();
        let hash_rng = vec![rng, mix_rng].concat();
        let mut out = Vec::new();

        for (i, b) in hash_rng.iter().enumerate() {
            let b = *b;
            let add = (mix_rng[i % mix_rng.len()] as usize) % (i + 1);
            let add = add as u8;
            let new_b = b.wrapping_add(add.wrapping_add(rng[i % rng.len()] % 8));
            out.push(new_b);
        }

        let mut salt = [0u8; 32];
        salt.copy_from_slice(&out[..32]);

        Salt::Salt(salt)
    }

    /// Returns the salt as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Salt::Salt(bytes) => bytes,
        }
    }

    /// Returns the salt as a vector of bytes.
    pub fn to_vec(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// Generates a new salt using a combination of random bytes from the thread and OS random number generators.
pub fn generate_salt() -> Salt {
    let rng = *Rng::thread_rng().as_bytes();
    let mix_rng = *Rng::osrng().as_bytes();
    let hash_rng = vec![rng, mix_rng].concat();
    let mut out = Vec::new();

    for (i, b) in hash_rng.iter().enumerate() {
        let b = *b;
        let add = (mix_rng[i % mix_rng.len()] as usize) % (i + 1);
        let add = add as u8;
        let new_b = b.wrapping_add(add.wrapping_add(rng[i % rng.len()] % 8));
        out.push(new_b);
    }

    let mut salt = [0u8; 32];
    salt.copy_from_slice(&out[..32]);

    Salt::Salt(salt)
}

/// Returns vector or byte slice as a salt data.
/// You can use this to turn a vector or byte slice into a salt.
pub trait AsSalt {
    fn as_salt(&self) -> Salt;
    fn as_salt_safe(&self) -> Result<Salt, String>;
}

impl AsSalt for &[u8] {
    fn as_salt(&self) -> Salt {
        assert!(self.len() == 32, "Salt input must be exactly 32 bytes");
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&self[..32]);
        Salt::Salt(arr)
    }

    fn as_salt_safe(&self) -> Result<Salt, String> {
        if self.len() != 32 {
            Err("Salt input must be exactly 32 bytes".to_string())
        } else {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&self[..32]);
            Ok(Salt::Salt(arr))
        }
    }
}

// -----------------------------------------------------

struct GaloisField {
    mul_table: [[u8; 256]; 256],
    inv_table: [u8; 256],
    irreducible_poly: u8,
}

impl GaloisField {
    fn new(irreducible_poly: u8) -> Self {
        let mut gf = Self {
            mul_table: [[0; 256]; 256],
            inv_table: [0; 256],
            irreducible_poly,
        };

        gf.initialize_tables();
        gf
    }

    fn initialize_tables(&mut self) {
        for i in 0..256 {
            for j in 0..256 {
                self.mul_table[i][j] = self.multiply(i as u8, j as u8);
            }
        }
        for i in 1..256 {
            for j in 1..256 {
                if self.mul_table[i][j] == 1 {
                    self.inv_table[i] = j as u8;
                }
            }
        }
    }

    fn multiply(&self, a: u8, b: u8) -> u8 {
        let mut p = 0;
        let mut a_val = a as u16;
        let mut b_val = b as u16;

        while a_val != 0 && b_val != 0 {
            if b_val & 1 != 0 {
                p ^= a_val as u8;
            }

            let high_bit_set = a_val & 0x80;
            a_val <<= 1;

            if high_bit_set != 0 {
                a_val ^= self.irreducible_poly as u16;
            }

            b_val >>= 1;
        }

        p as u8
    }

    fn fast_multiply(&self, a: u8, b: u8) -> u8 {
        self.mul_table[a as usize][b as usize]
    }

    fn inverse(&self, a: u8) -> Option<u8> {
        if a == 0 {
            None
        } else {
            Some(self.inv_table[a as usize])
        }
    }
}

fn triangle_mix_columns(
    data: &mut [u8],
    gf: &GaloisField,
    config: Config,
) -> Result<Vec<u8>, Errors> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?; // Builds Thread Pool for performance and resource usage optimization.

    pool.install(|| {
        data.par_chunks_exact_mut(3).for_each(|chunk| {
            let a = chunk[0];
            let b = chunk[1];
            let c = chunk[2];

            chunk[0] = gf.fast_multiply(3, a) ^ gf.fast_multiply(2, b) ^ c;
            chunk[1] = gf.fast_multiply(4, b) ^ c;
            chunk[2] = gf.fast_multiply(5, c);
        })
    });

    Ok(data.to_vec())
}

fn inverse_triangle_mix_columns(
    data: &mut [u8],
    gf: &GaloisField,
    config: Config,
) -> Result<Vec<u8>, Errors> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?; // Builds Thread Pool for performance and resource usage optimization.

    pool.install(|| {
        data.par_chunks_exact_mut(3).for_each(|chunk| {
            let a = chunk[0];
            let b = chunk[1];
            let c = chunk[2];

            let inv_5 = gf.inverse(5).unwrap_or(1);
            let c_prime = gf.fast_multiply(inv_5, c);

            let inv_4 = gf.inverse(4).unwrap_or(1);
            let b_prime = gf.fast_multiply(inv_4, b ^ gf.fast_multiply(1, c_prime));

            let inv_3 = gf.inverse(3).unwrap_or(1);
            let a_prime = gf.fast_multiply(inv_3, a ^ gf.fast_multiply(2, b_prime) ^ c_prime);

            chunk[0] = a_prime;
            chunk[1] = b_prime;
            chunk[2] = c_prime;
        })
    });

    Ok(data.to_vec())
}

fn xor_encrypt(nonce: &[u8], pwd: &[u8], input: &[u8]) -> Result<Vec<u8>, Errors> {
    let out = input
        .into_par_iter()
        .enumerate()
        .map(|(i, b)| {
            let masked = b ^ (nonce[i % nonce.len()] ^ pwd[i % pwd.len()]); // XOR the byte with the nonce and password
            let mut masked =
                masked.rotate_left((nonce[i % nonce.len()] ^ pwd[i % pwd.len()] % 8) as u32); // Rotate the byte left by the nonce value

            masked = masked.wrapping_add(pwd[i % pwd.len()]); // Add the password to the byte
            masked = masked.wrapping_add(nonce[i % nonce.len()]); // Add the nonce to the byte

            masked
        })
        .collect::<Vec<u8>>();

    match out.is_empty() {
        true => return Err(Errors::InvalidXor("Empty vector".to_string())),
        false => Ok(out),
    }
}

fn xor_decrypt(nonce: &[u8], pwd: &[u8], input: &[u8]) -> Result<Vec<u8>, Errors> {
    let out = input
        .into_par_iter()
        .enumerate()
        .map(|(i, b)| {
            let masked = b.wrapping_sub(nonce[i % nonce.len()]); // Subtract the nonce from the byte
            let masked = masked.wrapping_sub(pwd[i % pwd.len()]); // Subtract the password from the byte

            let masked =
                masked.rotate_right((nonce[i % nonce.len()] ^ pwd[i % pwd.len()] % 8) as u32); // Rotate the byte right by the nonce value

            masked ^ (nonce[i % nonce.len()] ^ pwd[i % pwd.len()]) // XOR the byte with the nonce and password
        })
        .collect::<Vec<u8>>();

    match out.is_empty() {
        true => return Err(Errors::InvalidXor("Empty vector".to_string())), // If out vector is empty then returns an Error
        false => Ok(out),
    }
}

fn mix_blocks(
    data: &mut Vec<u8>,
    nonce: &[u8],
    pwd: &[u8],
    config: Config,
) -> Result<Vec<u8>, Errors> {
    let nonce = blake3::hash(&[nonce, pwd].concat());
    let nonce = nonce.as_bytes();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?; // Builds Thread Pool for performance and resource usage optimization.

    if data.len().ct_eq(&3).unwrap_u8() == 1 {
        return Ok(data.to_vec()); // If data len <
    }

    let pool = pool.install(|| {
        data.into_par_iter()
            .enumerate()
            .map(|(i, byte)| {
                let n = nonce[i % nonce.len()];
                let mut byte = *byte;
                byte = byte.wrapping_add(n);
                byte = byte.rotate_right((n % 8) as u32); // Rotate the byte right by the nonce value
                byte ^= n; // XOR the byte with the nonce
                byte = byte.wrapping_add(n);

                byte
            })
            .collect::<Vec<u8>>() // While going through data changing bits, bits by bits
    });

    Ok(pool)
}

fn unmix_blocks(
    data: &mut Vec<u8>,
    nonce: &[u8],
    pwd: &[u8],
    config: Config,
) -> Result<Vec<u8>, Errors> {
    let nonce = blake3::hash(&[nonce, pwd].concat());
    let nonce = nonce.as_bytes();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?;

    if data.len().ct_eq(&3).unwrap_u8() == 1 {
        return Ok(data.to_vec());
    }

    let pool = pool.install(|| {
        data.into_par_iter()
            .enumerate()
            .map(|(i, byte)| {
                let n = nonce[i % nonce.len()];
                let mut byte = *byte;
                byte = byte.wrapping_sub(n);
                byte ^= n; // XOR the byte with the nonce
                byte = byte.rotate_left((n % 8) as u32); // Rotate the byte left by the nonce value
                byte = byte.wrapping_sub(n);

                byte
            })
            .collect::<Vec<u8>>()
    });

    Ok(pool)
}

fn derive_password_key(
    pwd: &[u8],
    salt: &[u8],
    custom_salt: Option<Salt>,
) -> Result<Vec<u8>, Errors> {
    if pwd.len().ct_eq(&32).unwrap_u8() != 1 {
        return Err(Errors::Argon2Failed("Invalid Password".to_string()));
    }

    let mut salt = SaltString::encode_b64(salt).map_err(|e| Errors::Argon2Failed(e.to_string()))?;

    if let Some(custom_salt) = custom_salt {
        salt = SaltString::encode_b64(custom_salt.as_bytes())
            .map_err(|e| Errors::Argon2Failed(e.to_string()))?;
    }

    let argon = Argon2::default();

    let mut out = vec![0u8; 32]; // 256-bit key
    argon
        .hash_password_into(pwd, salt.as_str().as_bytes(), &mut out)
        .map_err(|e| Errors::Argon2Failed(e.to_string()))?; // Hashing Password VIA Argon2

    Ok(out)
}

// TODO: Better key verification system via new dervition system; While Argon2 getting better salt Key will become more secure and easy to verify
fn verify_keys_constant_time(key1: &[u8], key2: &[u8]) -> Result<bool, Errors> {
    if key1.len().ct_eq(&key2.len()).unwrap_u8() != 1 {
        return Ok(false);
    }

    let result = key1.ct_eq(key2).unwrap_u8() == 1;
    Ok(result)
}

fn generate_inv_s_box(s_box: &[u8; 256]) -> [u8; 256] {
    let mut inv_s_box = [0u8; 256];
    for (i, &val) in s_box.iter().enumerate() {
        // Iterate over the s_box
        inv_s_box[val as usize] = i as u8; // Inverse the s_box
    }

    inv_s_box
}

fn generate_dynamic_sbox(nonce: &[u8], key: &[u8], cfg: Config) -> [u8; 256] {
    let mut sbox: [u8; 256] = [0; 256];
    for i in 0..256 {
        sbox[i] = i as u8;
    }

    let seed = match cfg.sbox {
        SboxTypes::PasswordBased => blake3::hash(&[key].concat()).as_bytes().to_vec(),
        SboxTypes::NonceBased => blake3::hash(&[nonce].concat()).as_bytes().to_vec(),
        SboxTypes::PasswordAndNonceBased => {
            blake3::hash(&[nonce, key].concat()).as_bytes().to_vec()
        }
    };

    for i in (1..256).rev() {
        let index = (seed[i % seed.len()] as usize + seed[(i * 7) % seed.len()] as usize) % (i + 1); // Generate a random index
        sbox.swap(i, index); // Swap the values in the sbox
    }

    sbox
}

fn in_s_bytes(data: &[u8], nonce: &[u8], pwd: &[u8], cfg: Config) -> Result<Vec<u8>, Errors> {
    let mut sbox = generate_dynamic_sbox(nonce, pwd, cfg); // Generate the sbox
    let inv_sbox = generate_inv_s_box(&sbox); // Generate the inverse sbox

    sbox.zeroize();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?;

    Ok(pool.install(|| data.par_iter().map(|b| inv_sbox[*b as usize]).collect())) // Inverse the sbox
}

fn s_bytes(data: &[u8], sbox: &[u8; 256], cfg: Config) -> Result<Vec<u8>, Errors> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?;

    Ok(pool.install(|| data.par_iter().map(|b| sbox[*b as usize]).collect())) // Apply the sbox
}

fn dynamic_sizes(data_len: usize) -> u32 {
    match data_len {
        0..1_000 => 14,
        1_000..10_000 => 24,
        10_000..100_000 => 64,
        100_000..1_000_000 => 128,
        1_000_000..10_000_000 => 4096,
        10_000_000..100_000_000 => 8096,
        100_000_000..1_000_000_000 => 16384,
        1_000_000_000..10_000_000_000 => 16384,
        10_000_000_000..100_000_000_000 => 32768,
        100_000_000_000..1_000_000_000_000 => 32768,
        1_000_000_000_000..10_000_000_000_000 => 65536,
        10_000_000_000_000..100_000_000_000_000 => 65536,
        100_000_000_000_000..1_000_000_000_000_000 => 1048576,
        1_000_000_000_000_000..10_000_000_000_000_000 => 1048576,
        10_000_000_000_000_000..100_000_000_000_000_000 => 2097152,
        100_000_000_000_000_000..1_000_000_000_000_000_000 => 2097152,
        1_000_000_000_000_000_000..10_000_000_000_000_000_000 => 4194304,
        _ => unreachable!(),
    }
}

// TODO: Better chunk generation
fn get_chunk_sizes(data_len: usize, nonce: &[u8], key: &[u8]) -> Vec<usize> {
    let mut sizes = Vec::new();
    let mut pos = 0;
    let hash = blake3::hash(&[nonce, key].concat());
    let seed = hash.as_bytes();

    let data_size = dynamic_sizes(data_len) as usize;

    while pos < data_len {
        let size = data_size + (seed[pos % seed.len()] as usize % 8); // Generate a random size for the chunk via Pos % Seed Lenght
        sizes.push(size.min(data_len - pos)); // Prevents code from unexpected errors and pushing data to sizes Vector
        pos += size;
    }

    sizes
}

fn dynamic_shift(
    data: &[u8],
    nonce: &[u8],
    password: &[u8],
    config: Config,
) -> Result<Vec<u8>, Errors> {
    let key = blake3::hash(&[nonce, password].concat())
        .as_bytes()
        .to_vec();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?;

    let chunk_sizes = get_chunk_sizes(data.len(), nonce, &key);

    let mut shifted = Vec::new();
    let mut cursor = 0;

    for (i, size) in chunk_sizes.iter().enumerate() {
        let mut chunk = data[cursor..cursor + size].to_vec();

        let rotate_by = (nonce[i % nonce.len()] % 8) as u32; // Rotate the byte left by the nonce value
        let xor_val = key[i % key.len()]; // XOR the byte with the nonce

        pool.install(|| {
            chunk.par_iter_mut().for_each(|b| {
                *b = b.rotate_left(rotate_by); // Rotate the byte left by the nonce value
                *b ^= xor_val; // XOR the byte with the nonce
            });

            shifted.par_extend(chunk);
            cursor += size; // Move the cursor to the next chunk
        })
    }

    shifted = shifted.iter().rev().cloned().collect::<Vec<u8>>();
    Ok(shifted)
}

fn dynamic_unshift(
    data: &[u8],
    nonce: &[u8],
    password: &[u8],
    config: Config,
) -> Result<Vec<u8>, Errors> {
    let data = data.iter().rev().cloned().collect::<Vec<u8>>();
    let key = blake3::hash(&[nonce, password].concat())
        .as_bytes()
        .to_vec();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.thread_num)
        .build()
        .map_err(|e| Errors::ThreadPool(e.to_string()))?;

    let chunk_sizes = get_chunk_sizes(data.len(), nonce, &key);

    let mut original = Vec::new();
    let mut cursor = 0;

    for (i, size) in chunk_sizes.iter().enumerate() {
        let mut chunk = data[cursor..cursor + size].to_vec();

        let rotate_by = (nonce[i % nonce.len()] % 8) as u32; // Rotate the byte left by the nonce value
        let xor_val = key[i % key.len()]; // XOR the byte with the nonce

        pool.install(|| {
            chunk.par_iter_mut().for_each(|b| {
                *b ^= xor_val; // XOR the byte with the nonce
                *b = b.rotate_right(rotate_by); // Rotate the byte right by the nonce value
            });

            original.par_extend(chunk);
            cursor += size; // Move the cursor to the next chunk
        })
    }

    Ok(original)
}

fn auto_dynamic_chunk_shift(
    data: &[u8],
    nonce: &[u8],
    password: &[u8],
    config: Config,
) -> Result<Vec<u8>, Errors> {
    match config.device {
        DeviceList::Cpu => Ok(dynamic_shift(data, nonce, password, config)?),
        DeviceList::Gpu => dynamic_shift_gpu(data, nonce, password),
        DeviceList::Auto => {
            if ocl::Platform::list().is_empty() {
                Ok(dynamic_shift(data, nonce, password, config)?)
            } else {
                dynamic_shift_gpu(data, nonce, password)
            }
        }
    }
}

fn auto_dynamic_chunk_unshift(
    data: &[u8],
    nonce: &[u8],
    password: &[u8],
    config: Config,
) -> Result<Vec<u8>, Errors> {
    match config.device {
        DeviceList::Cpu => Ok(dynamic_unshift(data, nonce, password, config)?),
        DeviceList::Gpu => dynamic_unshift_gpu(data, nonce, password),
        DeviceList::Auto => {
            if ocl::Platform::list().is_empty() {
                Ok(dynamic_unshift(data, nonce, password, config)?)
            } else {
                dynamic_unshift_gpu(data, nonce, password)
            }
        }
    }
}

// -----------------------------------------------------

fn encrypt(
    password: &str,
    data: &[u8],
    nonce: NonceData,
    config: Config,
    custom_salt: Option<Salt>,
    wrap_all: bool,
) -> Result<Vec<u8>, Errors> {
    let nonce = nonce.as_bytes();
    let mut password = derive_key(password, nonce);
    let mut pwd = derive_password_key(&password, nonce, custom_salt)?;

    password.zeroize();

    let mut out_vec = Vec::new();

    if wrap_all {
        out_vec.extend(nonce);
    }

    {
        let pwd = blake3::hash(b"atom-crypte-password");
        let pwd = *pwd.as_bytes();
        let encrypted_version = xor_encrypt(nonce, &pwd, VERSION)?;
        out_vec.extend(encrypted_version);
    }

    let mut s_block = generate_dynamic_sbox(nonce, &pwd, config);
    let mut mixed_data = mix_blocks(&mut s_bytes(data, &s_block, config)?, nonce, &pwd, config)?;
    let mut mixed_columns_data = triangle_mix_columns(
        &mut mixed_data,
        &GaloisField::new(config.gf_poly.value()),
        config,
    )?;

    mixed_data.zeroize();

    let mut shifted_data = s_bytes(
        &auto_dynamic_chunk_shift(&mixed_columns_data, nonce, &pwd, config)?,
        &s_block,
        config,
    )?;

    s_block.zeroize();
    mixed_columns_data.zeroize();

    let mut crypted = Vec::new();
    let mut round_data = shifted_data.to_vec();

    for i in 0..=config.rounds {
        let slice_end = std::cmp::min(i * 8, pwd.len());
        let key = blake3::hash(&pwd[..slice_end]);

        let key = *key.as_bytes();

        let crypted_chunks = round_data
            .par_chunks(dynamic_sizes(round_data.len()) as usize)
            .map(|data: &[u8]| {
                xor_encrypt(nonce, &key, &data).map_err(|e| Errors::InvalidXor(e.to_string()))
            })
            .collect::<Result<Vec<Vec<u8>>, Errors>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<u8>>();

        if i == config.rounds {
            crypted.extend(crypted_chunks);
        } else {
            round_data = crypted_chunks;
        }
    }

    shifted_data.zeroize();

    let mut mac_sha = Sha3_512::new();
    mac_sha.update(&crypted);
    mac_sha.update(blake3::hash(&xor_encrypt(nonce, &pwd, &data)?).as_bytes());
    let mac = mac_sha.finalize();

    pwd.zeroize();

    out_vec.extend(crypted);
    out_vec.extend(mac);

    if wrap_all {
        if custom_salt.is_some() {
            out_vec.extend(
                custom_salt
                    .ok_or(Errors::BuildFailed("Cannot Open Salt".to_string()))?
                    .as_bytes(),
            );
        } else {
            out_vec.extend(nonce);
        }
    }

    Ok(out_vec)
}

// -----------------------------------------------------

fn decrypt(
    password: &str,
    data: &[u8],
    nonce: Option<NonceData>,
    config: Config,
    custom_salt: Option<Salt>,
    wrap_all: bool,
) -> Result<Vec<u8>, Errors> {
    let (nonce_data, custom_salt) = if let Some(nonce) = nonce {
        (nonce, custom_salt)
    } else {
        let (_, custom_salt) = data.split_at(data.len() - 32);
        let (nonce, _) = data.split_at(32);

        (nonce.as_nonce(), Option::from(custom_salt.as_salt()))
    };

    let nonce_byte = nonce_data.as_bytes();

    let password_hash: [u8; 32] = derive_key(password, nonce_byte);
    let mut expected_password =
        derive_password_key(&derive_key(password, nonce_byte), nonce_byte, custom_salt)?;
    let mut pwd = derive_password_key(&password_hash, nonce_byte, custom_salt)?;

    if !verify_keys_constant_time(&pwd, &expected_password)? {
        pwd.zeroize();
        expected_password.zeroize();
        return Err(Errors::InvalidMac("Invalid key".to_string()));
    }

    expected_password.zeroize();

    if data.len() < 32 + VERSION.len() {
        return Err(Errors::InvalidMac("Data is too short".to_string()));
    }

    let version_len = VERSION.len();

    let mut wrapped = false;

    let (rest, encrypted_version) = if nonce.is_some() && !wrap_all {
        let (encrypted_version, rest) = data.split_at(version_len);

        (rest, encrypted_version)
    } else {
        let (_, rest) = data.split_at(32);
        let (encrypted_version, rest) = rest.split_at(version_len);

        wrapped = true;
        (rest, encrypted_version)
    };

    let version_pwd = blake3::hash(b"atom-crypte-password");
    let version_pwd = *version_pwd.as_bytes();
    let version = xor_decrypt(nonce_byte, &version_pwd, encrypted_version)?;
    let version_2 = xor_decrypt(nonce_byte, &pwd, encrypted_version)?;

    if !version.starts_with(b"atom-version") || !version_2.starts_with(b"atom-version") {
        if version.starts_with(b"atom-version") || version_2.starts_with(b"atom-version") {
        } else {
            pwd.zeroize();
            return Err(Errors::InvalidAlgorithm);
        }
    }

    let (crypted, mac_key) = if version_2.starts_with(b"atom-version:0x2") {
        let (crypted, mac_key) = rest.split_at(rest.len() - 32);

        (crypted, mac_key)
    } else if version.starts_with(b"atom-version:0x3") && wrapped {
        let (crypted, rest) = rest.split_at(rest.len() - 96);
        let (mac_key, _) = rest.split_at(64);

        (crypted, mac_key)
    } else {
        let (crypted, mac_key) = rest.split_at(rest.len() - 64);

        (crypted, mac_key)
    };

    let mut xor_decrypted = Vec::new();
    let mut round_data = Vec::from(crypted);

    for i in (0..=config.rounds).rev() {
        let slice_end = std::cmp::min(i * 8, pwd.len());
        let key = blake3::hash(&pwd[..slice_end]);

        let key = *key.as_bytes();

        let decrypted = round_data
            .to_vec()
            .par_chunks_mut(dynamic_sizes(round_data.len()) as usize)
            .map(|data: &mut [u8]| {
                xor_decrypt(nonce_byte, &key, data).map_err(|e| Errors::InvalidXor(e.to_string()))
            })
            .collect::<Result<Vec<Vec<u8>>, Errors>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<u8>>();

        if i == 0 {
            xor_decrypted.extend(decrypted);
        } else {
            round_data = decrypted
        }
    }

    let mut unshifted = auto_dynamic_chunk_unshift(
        &in_s_bytes(&xor_decrypted, nonce_byte, &pwd, config)?,
        nonce_byte,
        &pwd,
        config,
    )?;

    xor_decrypted.zeroize();

    let mut inversed_columns = inverse_triangle_mix_columns(
        &mut unshifted,
        &GaloisField::new(config.gf_poly.value()),
        config,
    )?;
    let mut unmixed = unmix_blocks(&mut inversed_columns, nonce_byte, &pwd, config)?;

    unshifted.zeroize();
    inversed_columns.zeroize();

    let mut decrypted_data = in_s_bytes(&unmixed, nonce_byte, &pwd, config)?;

    unmixed.zeroize();

    if version.starts_with(b"atom-version:0x2") {
        let mac = blake3::keyed_hash(
            blake3::hash(&crypted).as_bytes(),
            &xor_encrypt(nonce_byte, &pwd, &decrypted_data)?,
        ); // Generate a MAC for the data

        pwd.zeroize();

        if mac.as_bytes().ct_eq(mac_key).unwrap_u8() != 1 {
            // Check if the MAC is valid
            decrypted_data.zeroize();
            return Err(Errors::InvalidMac("Invalid authentication".to_string()));
        }
    } else {
        let mut mac_sha = Sha3_512::new();
        mac_sha.update(crypted);
        mac_sha.update(blake3::hash(&xor_encrypt(nonce_byte, &pwd, &decrypted_data)?).as_bytes());
        let mac = mac_sha.finalize();

        if mac.as_slice().ct_eq(mac_key).unwrap_u8() != 1 {
            // Check if the MAC is valid
            decrypted_data.zeroize();
            return Err(Errors::InvalidMac("Invalid authentication".to_string()));
        }
    }

    Ok(decrypted_data)
}

// -----------------------------------------------------

impl AtomCrypteBuilder {
    /// Creates a new instance of AtomCrypteBuilder.
    pub fn new() -> Self {
        Self {
            password: None,
            data: None,
            config: None,
            nonce: None,
            salt: None,
            wrap_all: false,
            benchmark: false,
        }
    }

    /// Sets the data to be encrypted.
    /// -  Recommended using '&' when using `Vector<u8>`.
    pub fn data(mut self, data: &[u8]) -> Self {
        self.data = Some(data.to_vec());
        self
    }

    /// Sets the configuration for the encryption.
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Sets the password for the encryption.
    pub fn password(mut self, password: &str) -> Self {
        self.password = Some(password.to_string());
        self
    }

    /// Sets the nonce for the encryption.
    pub fn nonce(mut self, nonce: NonceData) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Sets the salt for the encryption.
    pub fn salt(mut self, salt: Salt) -> Self {
        self.salt = Some(salt);
        self
    }

    /// Sets the wrap_all flag for the encryption.
    pub fn wrap_all(mut self, wrap_all: bool) -> Self {
        self.wrap_all = wrap_all;
        self
    }

    /// Sets the benchmark flag for the encryption.
    pub fn benchmark(mut self) -> Self {
        self.benchmark = true;
        self
    }

    /// Encrypts the data using the provided configuration, password, and nonce.
    /// - Recommended using at the end of build.
    ///
    /// # Errors
    /// Returns an error if any of the required fields are missing.
    ///
    /// # Recommendations
    /// - Use a strong password.
    /// - Use a unique nonce for each encryption.
    pub fn encrypt(self) -> Result<Vec<u8>, Errors> {
        let config = self
            .config
            .ok_or_else(|| Errors::BuildFailed("Missing Config".to_string()))?;
        let data = self
            .data
            .ok_or_else(|| Errors::BuildFailed("Missing Data".to_string()))?;
        let password = self
            .password
            .ok_or_else(|| Errors::BuildFailed("Missing Password".to_string()))?;
        let nonce = self
            .nonce
            .ok_or_else(|| Errors::BuildFailed("Missing Nonce".to_string()))?;
        let salt = self.salt;
        let benchmark = self.benchmark;
        let wrap_all = self.wrap_all;

        if benchmark {
            let start = Instant::now();
            let out = encrypt(password.as_str(), &data, nonce, config, salt, wrap_all);
            let duration = start.elapsed();
            println!("Encryption took {}ms", duration.as_millis());
            out
        } else {
            encrypt(password.as_str(), &data, nonce, config, salt, wrap_all)
        }
    }

    /// Decrypts the data using the provided configuration, password, and nonce.
    /// - Recommended using at the end of build.
    /// - Recommended not using with encryption in same builder.
    ///
    /// # Errors
    /// Returns an error if any of the required fields are missing.
    ///
    /// # Recommendations
    /// - Renew the nonce after each decryption.
    pub fn decrypt(self) -> Result<Vec<u8>, Errors> {
        let config = self
            .config
            .ok_or_else(|| Errors::BuildFailed("Missing Config".to_string()))?;
        let data = self
            .data
            .ok_or_else(|| Errors::BuildFailed("Missing Data".to_string()))?;
        let password = self
            .password
            .ok_or_else(|| Errors::BuildFailed("Missing Password".to_string()))?;
        let nonce = self.nonce;
        let salt = self.salt;
        let benchmark = self.benchmark;
        let wrap_all = self.wrap_all;

        if benchmark {
            let start = Instant::now();
            let out = decrypt(password.as_str(), &data, nonce, config, salt, wrap_all);
            let duration = start.elapsed();
            println!("Decryption took {}ms", duration.as_millis());
            out
        } else {
            decrypt(password.as_str(), &data, nonce, config, salt, wrap_all)
        }
    }
}
