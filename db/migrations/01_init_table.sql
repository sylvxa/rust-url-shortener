CREATE TABLE routes (
    route_id CHAR(6) PRIMARY KEY,
    destination TEXT NOT NULL,
    expires INTEGER NOT NULL
);