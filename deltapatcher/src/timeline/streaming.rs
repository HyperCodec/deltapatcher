use core::marker::PhantomData;
use deku::{ctx::Order, no_std_io as io};

pub use deku::{reader::Reader as BitReader, writer::Writer as BitWriter};

use super::*;

#[derive(Clone, Default, Debug)]
struct TimelineIndex {
    jump_table: Vec<usize>,
    tail: usize,
}

/// Random-access timeline backed by a seekable store (ex: [`File`][std::fs::File], [`Cursor<Vec<u8>>`][io::Cursor]).
pub struct SeekableTimeline<S, D, M> {
    store: S,
    index: TimelineIndex,
    _marker: PhantomData<(D, M)>
}

impl<S, D, M> SeekableTimeline<S, D, M>
where 
    D: Delta,
    S: Read + Seek,
{
    /// Open a new [`SeekableTimeline`] and fully index it based on the current store.
    pub fn open_readonly(store: S) -> io::Result<Self> {
        let mut s = Self {
            store,
            index: TimelineIndex::default(),
            _marker: PhantomData,
        };

        s.index_full()?;

        Ok(s)
    }
    
    /// Wipes and reindexes the entire timeline.
    pub fn index_full(&mut self) -> io::Result<()> {
        self.index.jump_table.clear();
        self.index.tail = 0;
        
        self.index_new()
    }

    /// Indexes any new commits added to the end of the store since the previous index.
    pub fn index_new(&mut self) -> io::Result<()> {
        let mut reader = BitReader::new(&mut self.store);
        reader.seek_bits(self.index.tail)?;
        while !reader.end() {
            let entry_len = u32::from_reader_with_ctx(&mut reader, Endian::Big)?;
            let entry_idx = reader.bits_read;

            self.index.jump_table.push(entry_idx);
            reader.skip_bits(entry_len as usize, Order::Lsb0)?;
        }

        self.index.tail = reader.bits_read;

        Ok(())
    }
}

impl<S, D, M, Ctx> TimelineRead<D, M, Ctx> for SeekableTimeline<S, D, M>
where 
    D: Delta,
    Commit<D, M>: for<'a> DekuReader<'a, Ctx>,
    S: Read + Seek,
{
    fn len(&self) -> usize {
        self.index.jump_table.len()
    }

    fn get_commit(&mut self, index: usize, ctx: Ctx) -> io::Result<Option<Commit<D, M>>> {
        if index >= self.len() {
            // out of bounds
            return Ok(None);
        }

        let entry_start = self.index.jump_table[index];

        let mut reader = BitReader::new(&mut self.store);
        reader.seek_bits(entry_start)?;

        let commit = Commit::<D, M>::from_reader_with_ctx(&mut reader, ctx)?;
        Ok(Some(commit))
    }
    
    fn iter(&mut self, ctx: Ctx) -> io::Result<impl Iterator<Item = io::Result<Commit<D, M>>>>
    where 
        Ctx: Copy
    {
        self.store.rewind()?;

        Ok(SeekableTimelineIterator {
            reader: BitReader::new(&mut self.store),
            tail: self.index.tail,
            ctx,
            _marker: PhantomData,
        })
    }
}

pub struct SeekableTimelineIterator<S, D, M, Ctx>
where 
    S: Read + Seek
{
    reader: BitReader<S>,
    tail: usize,
    ctx: Ctx,
    _marker: PhantomData<(D, M)>,
}

impl<S, D, M, Ctx> SeekableTimelineIterator<S, D, M, Ctx>
where 
    S: Read + Seek,
    D: Delta,
    Commit<D, M>: for<'a> DekuReader<'a, Ctx>,
    Ctx: Copy,
{
    fn next_unchecked(&mut self) -> io::Result<Commit<D, M>> {
        // skip the length prefix since it's not useful to us.
        self.reader.skip_bits(u32::SIZE_BITS, Order::Lsb0)?;
        Ok(Commit::<D, M>::from_reader_with_ctx(&mut self.reader, self.ctx)?)
    }
}

impl<S, D, M, Ctx> Iterator for SeekableTimelineIterator<S, D, M, Ctx>
where
    S: Read + Seek,
    D: Delta,
    Commit<D, M>: for<'a> DekuReader<'a, Ctx>,
    Ctx: Copy,
{
    type Item = io::Result<Commit<D, M>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.reader.bits_read >= self.tail {
            return None;
        }

        Some(self.next_unchecked())        
    }
}

pub trait TimelineRead<D, M, Ctx>
where 
    D: Delta,
    Commit<D, M>: for<'a> DekuReader<'a, Ctx>,
{
    fn len(&self) -> usize;
    fn get_commit(&mut self, index: usize, ctx: Ctx) -> io::Result<Option<Commit<D, M>>>;
    fn iter(&mut self, ctx: Ctx) -> io::Result<impl Iterator<Item = io::Result<Commit<D, M>>>>
    where 
        Ctx: Copy;
}

pub trait TimelineWrite<D, M, Ctx>
where
    D: Delta,
    Commit<D, M>: DekuWriter<Ctx>
{

}

pub trait GlobalBitSeek {
    fn seek_bits(&mut self, bits_from_start: usize) -> io::Result<()>;
}

impl<R: Read + Seek> GlobalBitSeek for BitReader<R> {
    fn seek_bits(&mut self, bits_from_start: usize) -> io::Result<()> {
        // let num_bytes = bits_from_start / 8;
        // let num_leftover = bits_from_start % 8;

        // self.seek(io::SeekFrom::Start(num_bytes))?;
        // self.skip_bits(num_leftover as usize, Order::Lsb0)?;
        // self.bits_read = bits_from_start as usize;
        self.rewind()?;
        self.skip_bits(bits_from_start, Order::Lsb0)?;

        Ok(())
    }
}

// TODO impl NaiveBitSeek for BitWriter<W>