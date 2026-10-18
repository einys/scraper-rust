// - 기존 `Box<dyn Error>`: **"아무 에러나 포장할 수 있지만, 다른 스레드로 보낼 순 없어."**
// - `DynError` (`Box<dyn Error + Send + Sync>`): **"아무 에러나 포장할 수 있고, 멀티스레드 비동기 환경에서도 안전하게 던질 수 있어."**
pub type DynError = Box<dyn std::error::Error + Send + Sync>;
pub type DynResult<T> = Result<T, DynError>;
