

pub fn rng_select<T>(list: &Vec<T>) -> T
    where T:Clone
{
    let mut rng = rand::thread_rng();
    let ix = rand::Rng::gen_range(&mut rng, 0..list.len());

    list[ix].clone()
}

pub fn roll(chance: f64) -> bool {
    let mut rng = rand::thread_rng();

    rand::Rng::gen_range(&mut rng, 0.0..1.0) < chance
}
