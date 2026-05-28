use chrono::{DateTime, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct FakeClock {
    pub time: std::sync::Mutex<DateTime<Utc>>,
}

impl FakeClock {
    pub fn new(time: DateTime<Utc>) -> Self {
        Self {
            time: std::sync::Mutex::new(time),
        }
    }

    pub fn advance(&self, duration: chrono::Duration) {
        let mut t = self.time.lock().unwrap();
        *t += duration;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        *self.time.lock().unwrap()
    }
}
