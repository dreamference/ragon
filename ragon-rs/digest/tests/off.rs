//! With the setting off nothing changes: `configure` never runs in this test binary, and
//! `inside` hands every output back as it came, in the same buffer, writing nothing.

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

#[test]
fn off_the_output_is_the_same_bytes_in_the_same_buffer() {
    assert!(!ragon_digest::enabled());
    let long: Vec<u8> = (0..5000).flat_map(|i| format!("error: line {i}\n").into_bytes()).collect();
    let binary: Vec<u8> = (0..100_000u32).map(|i| (i % 256) as u8).collect();
    for (command, output, exit_code) in [
        ("cargo test", long.clone(), Some(101)),
        ("cargo test", long.clone(), None),
        ("cat big.bin", binary.clone(), Some(0)),
        ("true", Vec::new(), Some(0)),
        ("echo hi", b"hi\n".to_vec(), Some(0)),
    ] {
        let expected = output.clone();
        let pointer = output.as_ptr();
        let returned = block_on(ragon_digest::inside(command, output, exit_code));
        assert_eq!(returned, expected, "{command}");
        assert_eq!(returned.as_ptr(), pointer, "{command}: a new buffer");
    }
}
