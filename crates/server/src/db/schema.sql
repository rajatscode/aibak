-- strat-club database schema

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    discord_id BIGINT UNIQUE NOT NULL,
    username TEXT NOT NULL,
    avatar_url TEXT,
    rating FLOAT NOT NULL DEFAULT 1500.0,
    rd FLOAT NOT NULL DEFAULT 350.0,
    volatility FLOAT NOT NULL DEFAULT 0.06,
    games_played INT NOT NULL DEFAULT 0,
    games_won INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS games (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    template TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'waiting',
    player_a UUID REFERENCES users(id),
    player_b UUID REFERENCES users(id),
    winner_id UUID REFERENCES users(id),
    turn INT NOT NULL DEFAULT 0,
    state_json JSONB,
    map_json JSONB,
    pick_options JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id UUID NOT NULL REFERENCES games(id),
    user_id UUID NOT NULL REFERENCES users(id),
    turn INT NOT NULL,
    orders_json JSONB NOT NULL,
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(game_id, user_id, turn)
);

CREATE TABLE IF NOT EXISTS turn_deadlines (
    game_id UUID NOT NULL REFERENCES games(id),
    turn INT NOT NULL,
    deadline TIMESTAMPTZ NOT NULL,
    PRIMARY KEY(game_id, turn)
);

CREATE TABLE IF NOT EXISTS rating_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    game_id UUID NOT NULL REFERENCES games(id),
    old_rating FLOAT NOT NULL,
    new_rating FLOAT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_games_status ON games(status);
CREATE INDEX IF NOT EXISTS idx_games_players ON games(player_a, player_b);
CREATE INDEX IF NOT EXISTS idx_orders_game ON orders(game_id, turn);
CREATE INDEX IF NOT EXISTS idx_rating_history_user ON rating_history(user_id);
