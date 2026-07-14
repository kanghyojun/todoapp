-- 사람이 부르는 짧은 코드. 무작위 4 글자, 소문자+숫자(0 1 o i l 제외).
-- 기존 행은 앱 시작 시(백필) 채운다. 그래서 nullable 로 두고, 값이 있는 행만
-- 유일성을 강제한다.
ALTER TABLE todos ADD COLUMN code TEXT;
CREATE UNIQUE INDEX idx_todos_code ON todos(code) WHERE code IS NOT NULL;
