//! Helper process for the crash test: opens a journal, writes preimages in a
//! tight loop, and never closes. The parent kills it at an arbitrary point.
//!
//! Usage: `crash_writer <path> <count>`
//!
//! It prints `ready` and flushes as soon as the journal is open, so the parent
//! knows the kill cannot happen before the first durable write.

use std::io::Write;

use conflux_domain::{IdentityQuality, Rect, WindowId, WindowMode};
use conflux_journal::{Journal, Preimage};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("path argument");
    let count: u64 = args
        .next()
        .unwrap_or_else(|| "100000".into())
        .parse()
        .unwrap();

    let mut journal = Journal::begin(&path, "crash-writer").expect("begin");
    println!("ready");
    std::io::stdout().flush().ok();

    for i in 0..count {
        let preimage = Preimage {
            id: WindowId {
                native_id: i,
                process_id: 1,
                process_start: 1,
            },
            display_native_id: "display-a".into(),
            display_quality: IdentityQuality::Strong,
            display_work_area: Rect::new(0, 0, 2560, 1400),
            display_scale: conflux_domain::Scale { x: 1.5, y: 1.5 },
            visible_rect: Rect::new(10, 20, 810, 620),
            outer_rect: Rect::new(3, 13, 817, 627),
            mode: WindowMode::Normal,
            topmost: false,
            native_placement: None,
        };
        journal.write_preimage(&preimage).expect("write");
    }

    // Reached only when `count` is small; keeps the process alive so a parent
    // that kills late still exercises the kill path.
    std::thread::sleep(std::time::Duration::from_secs(30));
}
