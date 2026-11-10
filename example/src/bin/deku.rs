use deltapatcher::{
    delta::{ArithmeticDelta, SliceDelta},
    timeline::{Commit, IncludeCache, StateCachedTimeline, Timeline},
    Diff, Patch,
};
use deku::prelude::*;
use deku::reader::Reader;
use deku::writer::Writer;
use std::io::Cursor;

fn main() {
    let basic_ctx = deku::ctx::Endian::Big;
    
    // Timeline items are Commit<Delta, ()>. Context expected: (DeltaCtx, MetaCtx)
    let timeline_ctx = (deku::ctx::Endian::Big, ());
    
    // StateCachedTimeline Context expected: (IncludeCache, TimelineCtx, StateCtx)
    let cached_ctx = (IncludeCache::CacheAndTimeline, (deku::ctx::Endian::Big, ()), deku::ctx::Endian::Big);

    // --- Serialize and deserialize a SliceDelta ---
    let initial = vec![1u32, 2, 3, 4, 5];
    let updated = vec![1u32, 9, 3, 10, 5];
    let slice_delta = updated.diff(&initial);

    let mut out_data = Vec::new();
    // FIX: Wrap `&mut out_data` in a Cursor so it implements `Seek`
    let mut cursor = Cursor::new(&mut out_data);
    let mut writer = Writer::new(&mut cursor);
    slice_delta.to_writer(&mut writer, basic_ctx).unwrap();
    
    println!("=== SliceDelta (Deku Bytes) ===");
    println!("{:02X?}\n", out_data);

    let mut cursor = Cursor::new(&out_data);
    let mut reader = Reader::new(&mut cursor);
    let deserialized = SliceDelta::<u32>::from_reader_with_ctx(&mut reader, basic_ctx).unwrap();

    let mut patched = initial.clone();
    patched.patch(&deserialized);
    assert_eq!(patched, updated);
    println!("Deserialized delta applied correctly: {patched:?}\n");

    // --- Serialize and deserialize an ArithmeticDelta ---
    let a: i32 = 42;
    let b: i32 = -17;
    let arith_delta: ArithmeticDelta<i32> = b.diff(&a);

    let mut out_data = Vec::new();
    let mut cursor = Cursor::new(&mut out_data);
    let mut writer = Writer::new(&mut cursor);
    arith_delta.to_writer(&mut writer, basic_ctx).unwrap();
    
    println!("=== ArithmeticDelta (Deku Bytes) ===");
    println!("{:02X?}\n", out_data);

    let mut cursor = Cursor::new(&out_data);
    let mut reader = Reader::new(&mut cursor);
    let deserialized = ArithmeticDelta::<i32>::from_reader_with_ctx(&mut reader, basic_ctx).unwrap();
    
    let mut patched = a;
    patched.patch(&deserialized);
    assert_eq!(patched, b);
    println!("Deserialized ArithmeticDelta applied correctly: {patched}\n");

    // --- Serialize and deserialize a Timeline ---
    let states = vec![0i32, 10, 35, 70];
    let timeline: Timeline<ArithmeticDelta<i32>> = states
        .windows(2)
        .map(|w| {
            let delta: ArithmeticDelta<i32> = w[1].diff(&w[0]);
            Commit::new(delta, ())
        })
        .collect();

    let mut out_data = Vec::new();
    let mut cursor = Cursor::new(&mut out_data);
    let mut writer = Writer::new(&mut cursor);
    // FIX: Pass the nested tuple context required by Timeline
    timeline.to_writer(&mut writer, timeline_ctx).unwrap();
    
    println!("=== Timeline (Deku Bytes) ===");
    println!("{:02X?}\n", out_data);

    let mut cursor = Cursor::new(&out_data);
    let mut reader = Reader::new(&mut cursor);
    let deserialized = Timeline::<ArithmeticDelta<i32>>::from_reader_with_ctx(&mut reader, timeline_ctx).unwrap();
    
    let rebuilt: i32 = deserialized.build_state_from_default(..);
    assert_eq!(rebuilt, *states.last().unwrap());
    println!("Deserialized Timeline rebuilt state correctly: {rebuilt}\n");

    // --- Serialize and deserialize a StateCachedTimeline ---
    let mut cached = StateCachedTimeline::new(2, 0i32);
    for delta in [5, 15, 25, 35].map(ArithmeticDelta) {
        cached.push(Commit::new(delta, ()));
    }

    let mut out_data = Vec::new();
    let mut cursor = Cursor::new(&mut out_data);
    let mut writer = Writer::new(&mut cursor);
    // FIX: Pass the 3-part nested tuple context required by StateCachedTimeline
    cached.to_writer(&mut writer, cached_ctx).unwrap();
    
    println!("=== StateCachedTimeline (Deku Bytes) ===");
    println!("{:02X?}\n", out_data);

    let mut cursor = Cursor::new(&out_data);
    let mut reader = Reader::new(&mut cursor);
    let deserialized = StateCachedTimeline::<i32, ArithmeticDelta<i32>>::from_reader_with_ctx(&mut reader, cached_ctx).unwrap();
    
    assert_eq!(deserialized.current_state(), cached.current_state());
    println!(
        "Deserialized StateCachedTimeline current state: {}\n",
        deserialized.current_state()
    );
}