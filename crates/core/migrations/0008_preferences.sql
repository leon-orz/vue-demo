-- 普通偏好按命名空间存储，不存放敏感凭据
CREATE TABLE IF NOT EXISTS preference (
    namespace TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
