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

-- ── League / Season system ──

CREATE TABLE IF NOT EXISTS seasons (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT false,
    config JSONB NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS season_standings (
    season_id INT NOT NULL REFERENCES seasons(id),
    user_id UUID NOT NULL REFERENCES users(id),
    rank_tier TEXT NOT NULL DEFAULT 'bronze',
    rank_points INT NOT NULL DEFAULT 0,
    wins INT NOT NULL DEFAULT 0,
    losses INT NOT NULL DEFAULT 0,
    streak INT NOT NULL DEFAULT 0,
    peak_rank_points INT NOT NULL DEFAULT 0,
    PRIMARY KEY (season_id, user_id)
);

CREATE TABLE IF NOT EXISTS match_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    game_id UUID NOT NULL REFERENCES games(id),
    season_id INT REFERENCES seasons(id),
    player_a UUID NOT NULL REFERENCES users(id),
    player_b UUID NOT NULL REFERENCES users(id),
    winner_id UUID REFERENCES users(id),
    player_a_rating_change FLOAT,
    player_b_rating_change FLOAT,
    player_a_rp_change INT,
    player_b_rp_change INT,
    turns_played INT,
    template TEXT,
    played_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_match_history_player ON match_history(player_a);
CREATE INDEX IF NOT EXISTS idx_match_history_player_b ON match_history(player_b);
CREATE INDEX IF NOT EXISTS idx_match_history_season ON match_history(season_id);

-- ── Arena tournament system ──

CREATE TABLE IF NOT EXISTS arenas (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    template TEXT NOT NULL,
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ NOT NULL,
    time_control_secs INT NOT NULL DEFAULT 300,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS arena_participants (
    arena_id UUID NOT NULL REFERENCES arenas(id),
    user_id UUID NOT NULL REFERENCES users(id),
    score INT NOT NULL DEFAULT 0,
    games_played INT NOT NULL DEFAULT 0,
    wins INT NOT NULL DEFAULT 0,
    current_streak INT NOT NULL DEFAULT 0,
    PRIMARY KEY (arena_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_arenas_time ON arenas(start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_arena_participants_arena ON arena_participants(arena_id);

-- ── Feedback system ──

CREATE TABLE IF NOT EXISTS feedback (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    content TEXT NOT NULL CHECK(char_length(content) <= 2000),
    upvotes INT NOT NULL DEFAULT 0,
    downvotes INT NOT NULL DEFAULT 0,
    resolved BOOLEAN NOT NULL DEFAULT false,
    resolved_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS feedback_votes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    feedback_id UUID NOT NULL REFERENCES feedback(id),
    direction INT NOT NULL CHECK(direction IN (-1, 1)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(user_id, feedback_id)
);

CREATE INDEX IF NOT EXISTS idx_feedback_upvotes ON feedback(upvotes DESC, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_feedback_votes_feedback ON feedback_votes(feedback_id);
