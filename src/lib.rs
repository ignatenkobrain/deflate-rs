#[cfg(test)]
extern crate flate2;
#[cfg(test)]
extern crate inflate;

mod huffman_table;
mod lz77;
mod chained_hash_table;
mod length_encode;
mod output_writer;
mod stored_block;
mod huffman_lengths;
use huffman_table::*;
use lz77::{LDPair, lz77_compress};
use huffman_lengths::write_huffman_lengths;

// TODO: Adding something in the unused bits here causes some issues
// Find out why
// The first bits of each block, which describe the type of the block
// `-TTF` - TT = type, 00 = stored, 01 = fixed, 10 = dynamic, 11 = reserved, F - 1 if final block
// `0000`;
const FIXED_FIRST_BYTE: u16 = 0b0000_0010;
const FIXED_FIRST_BYTE_FINAL: u16 = 0b0000_0011;
const DYNAMIC_FIRST_BYTE: u16 = 0b0000_0100;
const DYNAMIC_FIRST_BYTE_FINAL: u16 = 0b000_00101;

pub enum BType {
    NoCompression = 0b00,
    FixedHuffman = 0b01,
    DynamicHuffman = 0b10, // Reserved = 0b11, //Error
}

/// A quick implementation of a struct that writes bit data to a buffer
pub struct BitWriter {
    bit_position: u8,
    accumulator: u32,
    // We currently just write to a vector, but this should probably be
    // replaced with a writer later
    pub buffer: Vec<u8>,
}

impl BitWriter {
    pub fn new() -> BitWriter {
        BitWriter {
            bit_position: 0,
            accumulator: 0,
            buffer: Vec::new(),
        }
    }
    pub fn write_bits(&mut self, bits: u16, size: u8) {
        if size == 0 {
            return;
        }

        // self.accumulator |= (bits as u32) << (32 - size - self.bit_position);
        self.accumulator |= (bits as u32) << self.bit_position;
        self.bit_position += size;

        while self.bit_position >= 8 {
            // let byte = (self.accumulator >> 24) as u8;
            let byte = self.accumulator as u8;
            self.buffer.push(byte as u8);

            self.bit_position -= 8;
            // self.accumulator <<= 8;
            self.accumulator >>= 8;
        }
    }

    pub fn finish(&mut self) {
        if self.bit_position > 7 {
            // This should not happen.
            panic!("Error! Tried to finish bitwriter with more than 7 bits remaining!")
        }
        if self.bit_position != 0 {
            // println!("bit_position: {}, accumulator: {}", self.bit_position, self.accumulator);
            self.buffer.push(self.accumulator as u8);
        }
    }
}

// TODO: Use a trait here, and have implementations for each block type
struct EncoderState {
    huffman_table: huffman_table::HuffmanTable,
    writer: BitWriter,
    fixed: bool,
}

impl EncoderState {
    fn new(huffman_table: huffman_table::HuffmanTable) -> EncoderState {
        EncoderState {
            huffman_table: huffman_table,
            writer: BitWriter::new(),
            fixed: false,
        }
    }

    fn default() -> EncoderState {
        let mut ret = EncoderState::new(huffman_table::HuffmanTable::from_length_tables(&FIXED_CODE_LENGTHS,
                                                                                    &FIXED_CODE_LENGTHS_DISTANCE).unwrap());
            ret.fixed = true;
        ret
    }

    /// Encodes a literal value to the writer
    fn write_literal(&mut self, value: u8) {
        let code = self.huffman_table.get_literal(value);
        self.writer.write_bits(code.code, code.length);
    }

    fn write_ldpair(&mut self, value: LDPair) {
        match value {
            LDPair::Literal(l) => self.write_literal(l),
            LDPair::LengthDistance { length, distance } => {
                let ldencoded = self.huffman_table
                    .get_length_distance_code(length, distance)
                    .expect(&format!("Failed to get code for length: {}, distance: {}",
                                     length,
                                     distance));
                self.writer.write_bits(ldencoded.length_code.code, ldencoded.length_code.length);
                self.writer.write_bits(ldencoded.length_extra_bits.code,
                                       ldencoded.length_extra_bits.length);
                self.writer
                    .write_bits(ldencoded.distance_code.code, ldencoded.distance_code.length);
                self.writer.write_bits(ldencoded.distance_extra_bits.code,
                                       ldencoded.distance_extra_bits.length);
            }
            LDPair::BlockStart{is_final: _} => {
                panic!("Tried to write start of block, this should not be handled here!");
            }
        };
    }

    /// Write the start of a block
    fn write_start_of_block(&mut self, final_block: bool) {
        if final_block {
            // The final block has one bit flipped to indicate it's
            // the final one one
            if self.fixed {
                self.writer.write_bits(FIXED_FIRST_BYTE_FINAL, 3);
            } else {
                self.writer.write_bits(DYNAMIC_FIRST_BYTE_FINAL, 3);
            }
        } else {
            if self.fixed {
                self.writer.write_bits(FIXED_FIRST_BYTE, 3);
            } else {
                self.writer.write_bits(DYNAMIC_FIRST_BYTE, 3);
            }
        }
    }

    fn write_end_of_block(&mut self) {
        let code = self.huffman_table.get_end_of_block();
        // println!("End of block code: {:?}", code);
        self.writer.write_bits(code.code, code.length);
        // self.writer.finish();
    }

    /// Move and return the buffer from the writer
    pub fn take_buffer(&mut self) -> Vec<u8> {
        std::mem::replace(&mut self.writer.buffer, vec![])
    }

    pub fn flush(&mut self) {
        self.writer.finish();
    }
}

pub fn compress_data_fixed(input: &[u8]) -> Vec<u8> {
    // let block_length = 7;//BLOCK_SIZE as usize;

    let mut output = Vec::new();
    let mut state = EncoderState::default();
    let compressed = lz77_compress(input, chained_hash_table::WINDOW_SIZE).unwrap();
    let clen = compressed.len();

    //We currently don't split blocks, we should do this eventually
    state.write_start_of_block(true);
    for ld in compressed {
        //We ignore end of block here for now since there is no purpose of
        //splitting a full stream of data using fixed huffman data into blocks
        match ld {
            LDPair::BlockStart{is_final: _} =>
            (),
                _ => state.write_ldpair(ld),
        }
    }

    state.write_end_of_block();
    state.flush();

    output.extend(state.take_buffer());
    println!("Input length: {}, Compressed len: {}, Output length: {}",
             input.len(),
             clen,
             output.len());
    output
}

pub fn compress_data_dynamic(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    //NOTE: testing with default table first
    let mut state = EncoderState::new(huffman_table::HuffmanTable::from_length_tables(&FIXED_CODE_LENGTHS,
                                                                                    &FIXED_CODE_LENGTHS_DISTANCE).unwrap());

    let compressed = lz77_compress(input, chained_hash_table::WINDOW_SIZE).unwrap();

/*    state.write_start_of_block(first_block_is_final);

    write_huffman_lengths(&FIXED_CODE_LENGTHS, &FIXED_CODE_LENGTHS_DISTANCE, &mut state.writer);*/

    if let LDPair::BlockStart{..} = compressed[0] {} else {
        panic!("Compressed block doesn't start with block start! {:?}", compressed[0]);
    }
    //    assert_eq!(compressed[0], LDPair::BlockStart);

    for (n, ld) in compressed.into_iter().enumerate() {
        if let LDPair::BlockStart{is_final} = ld {
            if n > 0 {
                state.write_end_of_block();
            }
            state.write_start_of_block(is_final);
            write_huffman_lengths(&FIXED_CODE_LENGTHS, &FIXED_CODE_LENGTHS_DISTANCE, &mut state.writer)
        } else {
            state.write_ldpair(ld)
        }
    }

    state.write_end_of_block();
    state.flush();

    output.extend(state.take_buffer());

    output
}

pub fn compress_data(input: &[u8], btype: BType) -> Vec<u8> {
    match btype {
        BType::NoCompression => stored_block::compress_data_stored(input),
        BType::FixedHuffman => compress_data_fixed(input),
        BType::DynamicHuffman => compress_data_dynamic(input),
    }
}

#[cfg(test)]
mod test {

    /// Helper function to decompress into a `Vec<u8>`
    fn decompress_to_end(input: &[u8]) -> Vec<u8> {
         let mut inflater = super::inflate::InflateStream::new();
         let mut out = Vec::new();
         let mut n = 0;
         println!("input len {}", input.len());
         while n < input.len() {
         let (num_bytes_read, result) = inflater.update(&input[n..]).unwrap();
         println!("result len {}, bytes_read {}", result.len(), num_bytes_read);
         n += num_bytes_read;
         out.extend(result);
         }
         out
/*
        // Using flate2 instead of inflate, there seems to be some issue with inflate
        // for data longer than 399 bytes.
        use std::io::Read;
        use flate2::read::DeflateDecoder;

        let mut result = Vec::new();
        let mut e = DeflateDecoder::new(&input[..]);
        e.read_to_end(&mut result).unwrap();
        result*/
    }

    use super::*;


    #[test]
    fn test_no_compression_one_chunk() {
        let test_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let compressed = compress_data(&test_data, BType::NoCompression);
        let result = decompress_to_end(&compressed);
        assert_eq!(test_data, result);
    }

    #[test]
    fn test_no_compression_multiple_chunks() {
        let test_data = vec![32u8; 40000];
        let compressed = compress_data(&test_data, BType::NoCompression);
        let result = decompress_to_end(&compressed);
        assert_eq!(test_data, result);
    }

    #[test]
    fn test_no_compression_string() {
        let test_data = String::from("This is some text, this is some more text, this is even \
                                      more text, lots of text here.")
            .into_bytes();
        let compressed = compress_data(&test_data, BType::NoCompression);
        let result = decompress_to_end(&compressed);
        assert_eq!(test_data, result);
    }

    #[test]
    fn test_fixed_string_mem() {
        use std::str;
        // let test_data = b".......................BB";
        let test_data = String::from("                    GNU GENERAL PUBLIC LICENSE").into_bytes();
        let compressed = compress_data(&test_data, BType::FixedHuffman);

        let result = decompress_to_end(&compressed);
        println!("Output: `{}`", str::from_utf8(&result).unwrap());
        assert_eq!(test_data, result);
    }

    #[test]
    fn test_fixed_data() {

        let data = vec![190u8; 400];
        let compressed = compress_data(&data, BType::FixedHuffman);
        let result = decompress_to_end(&compressed);

        println!("data len: {}, result len: {}", data.len(), result.len());
        assert_eq!(data, result);
    }

    /// Test deflate example.
    ///
    /// Check if the encoder produces the same code as the example given by Mark Adler here:
    /// https://stackoverflow.com/questions/17398931/deflate-encoding-with-static-huffman-codes/17415203
    #[test]
    fn test_fixed_example() {
        let test_data = b"Deflate late";
        // let check =
        // [0x73, 0x49, 0x4d, 0xcb, 0x49, 0x2c, 0x49, 0x55, 0xc8, 0x49, 0x2c, 0x49, 0x5, 0x0];
        let check = [0x73, 0x49, 0x4d, 0xcb, 0x49, 0x2c, 0x49, 0x55, 0x00, 0x11, 0x00];
        let compressed = compress_data(test_data, BType::FixedHuffman);
        assert_eq!(&compressed, &check);
        let decompressed = decompress_to_end(&compressed);
        assert_eq!(&decompressed, test_data)
    }

    #[test]
    fn test_fixed_string_file() {
        use std::fs::File;
        use std::io::Read;
        use std::str;
        let mut input = Vec::new();

        let mut f = File::open("src/pg11.txt").unwrap();

        f.read_to_end(&mut input).unwrap();
        let compressed = compress_data(&input, BType::FixedHuffman);
        println!("Compressed len: {}", compressed.len());
        let result = decompress_to_end(&compressed);
        let out1 = str::from_utf8(&input).unwrap();
        let out2 = str::from_utf8(&result).unwrap();
        // println!("Orig:\n{}", out1);
        // println!("Compr:\n{}", out2);
        println!("Orig len: {}, out len: {}", out1.len(), out2.len());
        // Not using assert_eq here deliberately to avoid massive amounts of output spam
        assert!(input == result);
    }



    #[test]
    fn test_dynamic_string_mem() {
        use std::str;
        // let test_data = b".......................BB";
        let test_data = String::from("                    GNU GENERAL PUBLIC LICENSE").into_bytes();
        let compressed = compress_data(&test_data, BType::DynamicHuffman);

        let result = decompress_to_end(&compressed);
        println!("Output: `{}`", str::from_utf8(&result).unwrap());
        assert_eq!(test_data, result);
    }

    //#[test]
    fn _test_writer() {
        let mut w = super::BitWriter::new();
        // w.write_bits(super::FIXED_FIRST_BYTE_FINAL, 3);
        w.write_bits(0b0111_0100, 8);
        w.write_bits(0, 8);
        println!("FIXED_FIRST_BYTE_FINAL: {:#b}",
                 super::FIXED_FIRST_BYTE_FINAL);
        println!("BIT: {:#b}", w.buffer[0]);
    }
}
