//! Request heads as every front server parses them: whatever the bytes, the
//! parser returns a head, asks for more, or refuses — it never panics, and a
//! head it accepts survives every accessor the connection code uses.

#![no_main]

use bot_cooker::data::Data;
use bot_cooker::persona::Frontend;
use bot_cooker::wire::parse_head;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for frontend in Frontend::ALL {
        let wire = &Data::embedded().server(*frontend).wire;
        let mut buf = data.to_vec();
        // Pipelined requests: keep parsing what follows a complete head.
        while let Ok(Some(head)) = parse_head(&mut buf, wire) {
            let _ = head.path_and_query();
            let _ = head.wants_keep_alive();
            let _ = head.content_length();
            let _ = head.chunked();
            if buf.is_empty() {
                break;
            }
        }
    }
});
