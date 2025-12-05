module 0x2::t;

#[test]
fun test_move() {
    f();
}

fun f() {
    let x = 1;
    let y = x + 1;
    g(y);
}

fun g(z: u64) {
    abort z
}
