pub fn log2(n: u64) -> Option<u32> {
    let mut pow = 0;
    for k in 0..64 {
        if n >> k > 0 {
            continue;
        } else {
            if k == 0 {
                return None;
            }
            pow = k - 1;
            break;
        }
    }
    Some(pow)
}

#[test]
pub fn log2test() {
    let test = [0, 1, 4, 8, 9, 10, 15, 16];
    for i in test {
        let k = log2(i);
        println!("{} -> {:?}", i, k)
    }
}

#[test]
pub fn mo() {
    dbg!(1 + (2 << 1));
}
