use argon2::{Argon2, password_hash::SaltString};
use blake3::derive_key;
use rand::{TryRngCore, rngs::OsRng};
use rayon::prelude::*;
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::Zeroize;
static VERSION: &[u8] = b"atom-version:0x1";

pub struct Nonce;
pub struct AtomCrypte;

// # AtomCrypte Encryption Library
// A multi-layered, secure encryption library combining:
// - Blake3 hashing for fast and secure key derivation
// - Argon2 for password-based key derivation
// - Dynamic S-boxes for substitution operations
// - Chunk-based data transformation
// - Authenticated encryption with MAC validation
// - Constant-time operations to prevent timing attacks
// - Secure memory handling with zeroize
//
// This library implements a custom encryption protocol designed to provide
// multiple layers of security through various cryptographic techniques.

/// Represents different types of nonces used in the encryption process:
/// - TaggedNonce: Nonce combined with a user-provided tag
/// - HashedNonce: Cryptographically hashed nonce for extra randomness
/// - Nonce: Standard cryptographically secure random nonce
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NonceData {
    TaggedNonce([u8; 32]),
    HashedNonce([u8; 32]),
    Nonce([u8; 32]),
} // Multiple data types for future usage

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
}

impl NonceData {
    pub fn as_bytes(&self) -> &[u8; 32] {
        match self {
            NonceData::Nonce(n) | NonceData::HashedNonce(n) | NonceData::TaggedNonce(n) => n,
        }
    }
    pub fn to_vec(&self) -> Vec<u8> {
        match self {
            NonceData::Nonce(n) | NonceData::HashedNonce(n) | NonceData::TaggedNonce(n) => {
                n.to_vec()
            }
        }
    }
}

pub trait AsNonce {
    fn as_nonce(&self) -> Result<NonceData, Errors>;
}

impl TryFrom<&[u8]> for NonceData {
    type Error = Errors;

    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        if slice.len() != 32 {
            return Err(Errors::InvalidNonce(
                "Nonce must be exactly 32 bytes".to_string(),
            ));
        }

        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(slice);
        Ok(NonceData::Nonce(nonce))
    }
}

impl TryFrom<Vec<u8>> for NonceData {
    type Error = Errors;

    fn try_from(vec: Vec<u8>) -> Result<Self, Self::Error> {
        if vec.len() != 32 {
            return Err(Errors::InvalidNonce(
                "Nonce must be exactly 32 bytes".to_string(),
            ));
        }

        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&vec);
        Ok(NonceData::Nonce(nonce))
    }
}

impl AsNonce for [u8] {
    fn as_nonce(&self) -> Result<NonceData, Errors> {
        NonceData::try_from(self)
    }
}

impl AsNonce for Vec<u8> {
    fn as_nonce(&self) -> Result<NonceData, Errors> {
        NonceData::try_from(self.as_slice())
    }
}

impl Nonce {
    pub fn hashed_nonce() -> Result<NonceData, Errors> {
        let mut nonce = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|e| Errors::InvalidNonce(e.to_string()))?; // Generate a 32 byte nonce using OsRng

        Ok(NonceData::HashedNonce(*blake3::hash(&nonce).as_bytes())) // Hash the nonce to get a 32 byte more random nonce (Extra Security)
    }

    pub fn tagged_nonce(tag: &[u8]) -> Result<NonceData, Errors> {
        let mut nonce = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|e| Errors::InvalidNonce(e.to_string()))?; // Generate a 32 byte nonce using OsRng

        Ok(NonceData::TaggedNonce(
            *blake3::hash(&[&nonce, tag].concat()).as_bytes(),
        )) // Hash the nonce to get a 32 byte more random nonce (Extra Security)
    }

    pub fn nonce() -> Result<NonceData, Errors> {
        let mut nonce = [0u8; 32];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|e| Errors::InvalidNonce(e.to_string()))?; // Generate a 32 byte nonce using OsRng

        Ok(NonceData::Nonce(nonce)) // Return the nonce
    }
}

fn xor_encrypt(nonce: &[u8], pwd: &[u8], input: &[u8]) -> Result<Vec<u8>, Errors> {
    let out = input
        .par_iter()
        .enumerate()
        .map(|(i, b)| {
            let masked = b ^ (nonce[i % nonce.len()] ^ pwd[i % pwd.len()]); // XOR the byte with the nonce and password
            let mut masked =
                masked.rotate_left((nonce[i % nonce.len()] ^ pwd[i % pwd.len()] % 8) as u32); // Rotate the byte left by the nonce value

            masked = masked.wrapping_add(nonce[i % nonce.len()]); // Add the nonce to the byte
            masked = masked.wrapping_add(pwd[i % pwd.len()]); // Add the password to the byte

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
        .par_iter()
        .enumerate()
        .map(|(i, b)| {
            let masked = b.wrapping_sub(pwd[i % pwd.len()]); // Subtract the password from the byte
            let masked = masked.wrapping_sub(nonce[i % nonce.len()]); // Subtract the nonce from the byte

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

fn mix_blocks(data: &mut Vec<u8>, nonce: &[u8], pwd: &[u8]) -> Result<Vec<u8>, Errors> {
    let nonce = blake3::hash(&[nonce, pwd].concat());
    let nonce = nonce.as_bytes();
    let thread = num_cpus::get() / 2;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread)
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

fn unmix_blocks(data: &mut Vec<u8>, nonce: &[u8], pwd: &[u8]) -> Result<Vec<u8>, Errors> {
    let nonce = blake3::hash(&[nonce, pwd].concat());
    let nonce = nonce.as_bytes();
    let thread = num_cpus::get() / 2;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread)
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

fn derive_password_key(pwd: &[u8], salt: &[u8]) -> Result<Vec<u8>, Errors> {
    if pwd.len().ct_eq(&32).unwrap_u8() != 1 {
        return Err(Errors::Argon2Failed("Invalid Password".to_string()));
    }

    // TODO: Better Salt
    let salt = SaltString::encode_b64(salt).map_err(|e| Errors::Argon2Failed(e.to_string()))?;
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

fn generate_dynamic_sbox(nonce: &[u8], key: &[u8]) -> [u8; 256] {
    let mut sbox: [u8; 256] = [0; 256];
    for i in 0..256 {
        sbox[i] = i as u8;
    }

    let seed = blake3::hash(&[nonce, key].concat()).as_bytes().to_vec();

    for i in (1..256).rev() {
        let index = (seed[i % seed.len()] as usize + seed[(i * 7) % seed.len()] as usize) % (i + 1); // Generate a random index
        sbox.swap(i, index); // Swap the values in the sbox
    }

    sbox
}

fn in_s_bytes(data: &[u8], nonce: &[u8], pwd: &[u8]) -> Vec<u8> {
    let mut sbox = generate_dynamic_sbox(nonce, pwd); // Generate the sbox
    let inv_sbox = generate_inv_s_box(&sbox); // Generate the inverse sbox

    sbox.zeroize();

    data.par_iter().map(|b| inv_sbox[*b as usize]).collect() // Inverse the sbox
}

fn s_bytes(data: &[u8], sbox: &[u8; 256]) -> Vec<u8> {
    data.par_iter().map(|b| sbox[*b as usize]).collect() // Apply the sbox
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
        let size = 4 + (seed[pos % seed.len()] as usize % data_size); // Generate a random size for the chunk via Pos % Seed Lenght
        sizes.push(size.min(data_len - pos)); // Prevents code from unexpected errors and pushing data to sizes Vector
        pos += size;
    }

    sizes
}

fn dynamic_chunk_shift(data: &[u8], nonce: &[u8], password: &[u8]) -> Vec<u8> {
    let key = blake3::hash(&[nonce, password].concat())
        .as_bytes()
        .to_vec();

    let chunk_sizes = get_chunk_sizes(data.len(), nonce, &key);

    let mut shifted = Vec::new();
    let mut cursor = 0;

    for (i, size) in chunk_sizes.iter().enumerate() {
        let mut chunk = data[cursor..cursor + size].to_vec();

        let rotate_by = (nonce[i % nonce.len()] % 8) as u32; // Rotate the byte left by the nonce value
        let xor_val = key[i % key.len()]; // XOR the byte with the nonce

        chunk.par_iter_mut().for_each(|b| {
            *b = b.rotate_left(rotate_by); // Rotate the byte left by the nonce value
            *b ^= xor_val; // XOR the byte with the nonce
        });

        shifted.par_extend(chunk);
        cursor += size; // Move the cursor to the next chunk
    }

    shifted = shifted.iter().rev().cloned().collect::<Vec<u8>>();
    shifted
}

fn dynamic_chunk_unshift(data: &[u8], nonce: &[u8], password: &[u8]) -> Vec<u8> {
    let data = data.iter().rev().cloned().collect::<Vec<u8>>();
    let key = blake3::hash(&[nonce, password].concat())
        .as_bytes()
        .to_vec();
    let chunk_sizes = get_chunk_sizes(data.len(), nonce, &key);

    let mut original = Vec::new();
    let mut cursor = 0;

    for (i, size) in chunk_sizes.iter().enumerate() {
        let mut chunk = data[cursor..cursor + size].to_vec();

        let rotate_by = (nonce[i % nonce.len()] % 8) as u32; // Rotate the byte left by the nonce value
        let xor_val = key[i % key.len()]; // XOR the byte with the nonce

        chunk.par_iter_mut().for_each(|b| {
            *b ^= xor_val; // XOR the byte with the nonce
            *b = b.rotate_right(rotate_by); // Rotate the byte right by the nonce value
        });

        original.par_extend(chunk);
        cursor += size; // Move the cursor to the next chunk
    }

    original
}

impl AtomCrypte {
    pub fn encrypt(pwd: &str, data: &[u8], nonce: NonceData) -> Result<Vec<u8>, Errors> {
        let nonce = nonce.as_bytes();

        let mut password = derive_key(pwd, nonce);
        let mut pwd = derive_password_key(&password, nonce)?;

        password.zeroize();

        let mut out_vec = Vec::new();

        let encrypted_version = xor_encrypt(nonce, &pwd, VERSION)?;
        out_vec.extend(encrypted_version);

        let mut s_block = generate_dynamic_sbox(nonce, &pwd);
        let mut mixed_data = mix_blocks(&mut s_bytes(data, &s_block), nonce, &pwd)?;
        let mut shifted_data = s_bytes(&dynamic_chunk_shift(&mixed_data, nonce, &pwd), &s_block);

        s_block.zeroize();
        mixed_data.zeroize();

        let crypted = shifted_data
            .par_chunks(dynamic_sizes(shifted_data.len()) as usize)
            .map(|data: &[u8]| {
                xor_encrypt(nonce, &pwd, &data).map_err(|e| Errors::InvalidXor(e.to_string()))
            })
            .collect::<Result<Vec<Vec<u8>>, Errors>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<u8>>();

        shifted_data.zeroize();

        let mac = *blake3::keyed_hash(
            blake3::hash(&crypted).as_bytes(),
            &xor_encrypt(nonce, &pwd, &data)?,
        )
        .as_bytes(); // Generate a MAC for the data

        pwd.zeroize();

        out_vec.extend(crypted);
        out_vec.extend(mac);

        Ok(out_vec)
    }

    pub fn decrypt(pwd: &str, data: &[u8], nonce: NonceData) -> Result<Vec<u8>, Errors> {
        let nonce = nonce.as_bytes();

        let password: [u8; 32] = derive_key(pwd, nonce);
        let mut expected_password = derive_password_key(&derive_key(pwd, nonce), nonce)?;
        let mut pwd = derive_password_key(&password, nonce)?;

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
        let (encrypted_version, rest) = data.split_at(version_len);
        let (crypted, mac_key) = rest.split_at(rest.len() - 32);

        {
            let version = xor_decrypt(nonce, &pwd, encrypted_version)?;

            if !version.starts_with(b"atom-version") {
                pwd.zeroize();
                return Err(Errors::InvalidAlgorithm);
            }
        }

        let mut xor_decrypted = crypted
            .to_vec()
            .par_chunks_mut(dynamic_sizes(crypted.len()) as usize)
            .map(|data: &mut [u8]| {
                xor_decrypt(nonce, &pwd, data).map_err(|e| Errors::InvalidXor(e.to_string()))
            })
            .collect::<Result<Vec<Vec<u8>>, Errors>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<u8>>();

        let mut unshifted =
            dynamic_chunk_unshift(&in_s_bytes(&xor_decrypted, nonce, &pwd), nonce, &pwd);

        xor_decrypted.zeroize();

        let mut unmixed = unmix_blocks(&mut unshifted, nonce, &pwd)?;

        unshifted.zeroize();

        let mut decrypted_data = in_s_bytes(&unmixed, nonce, &pwd);

        unmixed.zeroize();

        let mac = blake3::keyed_hash(
            blake3::hash(&crypted).as_bytes(),
            &xor_encrypt(nonce, &pwd, &decrypted_data)?,
        ); // Generate a MAC for the data

        pwd.zeroize();

        if mac.as_bytes().ct_eq(mac_key).unwrap_u8() != 1 {
            // Check if the MAC is valid
            decrypted_data.zeroize();
            return Err(Errors::InvalidMac("Invalid authentication".to_string()));
        }

        Ok(decrypted_data)
    }
}
