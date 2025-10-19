#[derive(Debug, Clone)]
pub struct UserData {
    pub user_name: String,
    pub page_count: u32,
    pub fav_count: u32,
}

impl UserData {
    pub fn new(user_name: String, page_count: u32, fav_count: u32) -> Self {
        Self {
            user_name,
            page_count,
            fav_count,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.page_count < 1
    }
}
