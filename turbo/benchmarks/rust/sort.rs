fn main() {
    let mut arr = [0i64; 200];
    for i in 0..200 { arr[i] = (200 - i) as i64; }
    let n = 200;
    for _ in 0..n {
        for j in 0..n - 1 {
            if arr[j] > arr[j + 1] { arr.swap(j, j + 1); }
        }
    }
    println!("{}\n{}", arr[0], arr[199]);
}
