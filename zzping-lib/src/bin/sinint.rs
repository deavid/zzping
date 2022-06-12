use zzping_lib::sin_integral;

fn main() {
    for x in -1000..1000 {
        let x = x as f64 / 100.0;
        let y = sin_integral::sinc_int_norm(x);
        println!("{:.2} => {:.4}", x, y);
    }
}
