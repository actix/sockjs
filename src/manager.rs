


pub trait SessionManager {

    /// Get or create session
    fn get(&self);

    /// Try to aquire existing session
    fn acquire(&self);

    /// Relese existing session
    fn release(&self);
}


pub struct SM {}

impl SM {
    pub fn new() -> Self {
        SM{}
    }
}

impl SessionManager for SM {
    fn get(&self) {
        unimplemented!()
    }
    fn acquire(&self) {
        unimplemented!()
    }
    fn release(&self) {
        unimplemented!()
    }
}
