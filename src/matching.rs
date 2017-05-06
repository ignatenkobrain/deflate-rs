use std::cmp;

use chained_hash_table::{ChainedHashTable, WINDOW_SIZE};
use huffman_table;

const MAX_MATCH: usize = huffman_table::MAX_MATCH as usize;
const MIN_MATCH: usize = huffman_table::MIN_MATCH as usize;

/// Get the length of the checked match
/// The function returns number of bytes at and including `current_pos` that are the same as the
/// ones at `pos_to_check`
fn get_match_length(data: &[u8], current_pos: usize, pos_to_check: usize) -> usize {
    // Unsafe version for comparison
    // This doesn't actually make it much faster

    // use std::mem::transmute_copy;

    // let mut counter = 0;
    // let max = cmp::min(data.len() - current_pos, MAX_MATCH);

    // unsafe {
    //     let mut cur = data.as_ptr().offset(current_pos as isize);
    //     let mut tc = data.as_ptr().offset(pos_to_check as isize);
    //     while (counter < max) &&
    //           (transmute_copy::<u8, u32>(&*cur) == transmute_copy::<u8, u32>(&*tc)) {
    //         counter += 4;
    //         cur = cur.offset(4);
    //         tc = tc.offset(4);
    //     }
    //     if counter > 3 {
    //         cur = cur.offset(-4);
    //         tc = tc.offset(-4);
    //         counter -= 4;
    //     }
    //     while counter < max && *cur == *tc {
    //         counter += 1;
    //         cur = cur.offset(1);
    //         tc = tc.offset(1);
    //     }
    // }

    //    counter
    data[current_pos..]
        .iter()
        .zip(data[pos_to_check..].iter())
        .take(MAX_MATCH)
        .take_while(|&(&a, &b)| a == b)
        .count()
}

/// Try finding the position and length of the longest match in the input data.
/// # Returns
/// (length, distance from position)
/// If no match is found that was better than `prev_length` or at all, or we are at the start,
/// the length value returned will be 2.
///
/// # Arguments:
/// `data`: The data to search in.
/// `hash_table`: Hash table to use for searching.
/// `position`: The position in the data to match against.
/// `prev_length`: The length of the previous `longest_match` check to compare against.
/// `max_hash_checks`: The maximum number of matching hash chain positions to check.
pub fn longest_match(data: &[u8],
                     hash_table: &ChainedHashTable,
                     position: usize,
                     prev_length: usize,
                     max_hash_checks: u16)
                     -> (usize, usize) {

    // debug_assert_eq!(position, hash_table.current_head() as usize);

    // If we are at the start, we already have a match at the maximum length,
    // or we can't grow further, we stop here.
    if position == 0 || prev_length >= MAX_MATCH || position + prev_length >= data.len() {
        return (0, 0);
    }

    let limit = if position > WINDOW_SIZE {
        position - WINDOW_SIZE
    } else {
        0
    };

    // Make sure the length is at least one to simplify the matching code, as
    // otherwise the matching code might underflow.
    let prev_length = if prev_length < 1 { 1 } else { prev_length };

    let max_length = cmp::min((data.len() - position), MAX_MATCH);

    // The position in the hash chain we are currently checking.
    let mut current_head = position;

    // The best match length we've found so far, and it's distance.
    let mut best_length = prev_length;
    let mut best_distance = 0;

    // The position of the previous value in the hash chain.
    let mut prev_head;

    for _ in 0..max_hash_checks {
        prev_head = current_head;
        current_head = hash_table.get_prev(current_head) as usize;
        if current_head >= prev_head || current_head < limit {
            // If the current hash chain value refers to itself, or is referring to
            // a value that's higher (we only move backwars through the chain),
            // we are at the end and can stop.
            break;
        }

        // We only check further if the match length can actually increase
        // Checking if the end byte and the potential next byte matches is generally
        // more likely to give a quick answer rather than checking from the start first, given
        // that the hashes match.
        // If there is no previous match, best_length will be 1 and the two first bytes will
        // be checked instead.
        // Since we've made sure best_length is always at least 1, this shouldn't underflow.
        if data[position + best_length - 1..position + best_length + 1] ==
           data[current_head + best_length - 1..current_head + best_length + 1] {
            // Actually check how many bytes match.
            // At the moment this will check the two bytes we just checked again,
            // though adding code for skipping these bytes may not result in any speed
            // gain due to the added complexity.
            let length = get_match_length(data, position, current_head);
            if length > best_length && length >= MIN_MATCH {
                best_length = length;
                best_distance = position - current_head;
                if length == max_length {
                    // We are at the max length, so there is no point
                    // searching any longer
                    break;
                }
            }
        }
    }

    let r = if best_length > prev_length {
        best_length
    } else {
        0
    };

    (r, best_distance)
}

// Get the longest match from the current position of the hash table.
#[inline]
#[cfg(test)]
pub fn longest_match_current(data: &[u8], hash_table: &ChainedHashTable) -> (usize, usize) {
    use compression_options::MAX_HASH_CHECKS;
    longest_match(data,
                  hash_table,
                  hash_table.current_head() as usize,
                  MIN_MATCH as usize - 1,
                  MAX_HASH_CHECKS)
}

#[cfg(test)]
mod test {
    use chained_hash_table::{filled_hash_table, HASH_BYTES, ChainedHashTable};
    use super::{get_match_length, longest_match};

    /// Test that match lengths are calculated correctly
    #[test]
    fn match_length() {
        let test_arr = [5u8, 5, 5, 5, 5, 9, 9, 2, 3, 5, 5, 5, 5, 5];
        let l = get_match_length(&test_arr, 9, 0);
        assert_eq!(l, 5);
        let l2 = get_match_length(&test_arr, 9, 7);
        assert_eq!(l2, 0);
        let l3 = get_match_length(&test_arr, 10, 0);
        assert_eq!(l3, 4);
    }

    /// Test that we get the longest of the matches
    #[test]
    fn get_longest_match() {
        let test_data = b"xTest data, Test_data,zTest data";
        let hash_table = filled_hash_table(&test_data[..23 + 1 + HASH_BYTES - 1]);

        let (length, distance) = super::longest_match_current(test_data, &hash_table);

        // We check that we get the longest match, rather than the shorter, but closer one.
        assert_eq!(distance, 22);
        assert_eq!(length, 9);
        let test_arr2 = [10u8, 10, 10, 10, 10, 10, 10, 10, 2, 3, 5, 10, 10, 10, 10, 10];
        let hash_table = filled_hash_table(&test_arr2[..HASH_BYTES + 1 + 1 + 2]);
        let (length, distance) = super::longest_match_current(&test_arr2, &hash_table);

        assert_eq!(distance, 1);
        assert_eq!(length, 4);
    }

    /// Make sure we can get a match at index zero
    #[test]
    fn match_index_zero() {
        let test_data = b"AAAAAAA";

        let mut hash_table = ChainedHashTable::from_starting_values(test_data[0], test_data[1]);
        for (n, &b) in test_data[2..5].iter().enumerate() {
            hash_table.add_hash_value(n, b);
        }

        let (match_length, match_dist) = longest_match(test_data, &hash_table, 2, 0, 4096);

        assert_eq!(match_dist, 1);
        assert!(match_length > 2);
    }
}
