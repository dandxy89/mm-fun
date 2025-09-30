use bytes::BytesMut;
use crossbeam_queue::ArrayQueue;

const BUFFER_SIZE: usize = 64 * 1024;

pub struct BufferPool {
    pool: ArrayQueue<BytesMut>,
}

impl BufferPool {
    pub fn new(capacity: usize, buffer_size: usize) -> Self {
        let pool = ArrayQueue::new(capacity);
        for _ in 0..capacity {
            let buffer = BytesMut::with_capacity(buffer_size);
            let _ = pool.push(buffer);
        }
        Self { pool }
    }

    pub fn get(&self) -> BytesMut {
        self.pool.pop().unwrap_or_else(|| BytesMut::with_capacity(BUFFER_SIZE))
    }

    pub fn return_buffer(&self, mut buffer: BytesMut) {
        buffer.clear();
        let _ = self.pool.push(buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool() {
        let pool = BufferPool::new(10, 1024);
        let buffer = pool.get();
        assert_eq!(buffer.capacity(), 1024);
        pool.return_buffer(buffer);
    }
}
