-- Navigation items (top nav bar)
CREATE TABLE nav (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated DATETIME DEFAULT CURRENT_TIMESTAMP,
  label TEXT NOT NULL,
  path TEXT NOT NULL,
  position INTEGER NOT NULL,
  active BOOLEAN DEFAULT 1
);

-- Tutorial chapters (e.g., "1. Basics", "2. Types", etc.)
CREATE TABLE chapters (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated DATETIME DEFAULT CURRENT_TIMESTAMP,
  nav INTEGER NOT NULL,
  title TEXT NOT NULL,
  position INTEGER NOT NULL,
  active BOOLEAN DEFAULT 1,
  FOREIGN KEY (nav) REFERENCES nav(id)
);

-- Tutorial topics (individual lessons like "Hello World", "Variables", etc.)
CREATE TABLE topics (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated DATETIME DEFAULT CURRENT_TIMESTAMP,
  chapter INTEGER NOT NULL,
  title TEXT NOT NULL,
  slug TEXT UNIQUE NOT NULL,
  -- URL-friendly ID like 'hello-world'
  description TEXT,
  explanation TEXT,
  position INTEGER NOT NULL,
  active BOOLEAN DEFAULT 1,
  FOREIGN KEY (chapter) REFERENCES chapters(id)
);

-- Code snippets for each topic (supports multiple tabs)
CREATE TABLE snippets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated DATETIME DEFAULT CURRENT_TIMESTAMP,
  topic INTEGER NOT NULL,
  code TEXT NOT NULL,
  position INTEGER NOT NULL,
  -- Tab order (1, 2, 3, etc.)
  FOREIGN KEY (topic) REFERENCES topics(id)
);

-- Migration metadata
CREATE TABLE migrations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created DATETIME DEFAULT CURRENT_TIMESTAMP,
  updated DATETIME DEFAULT CURRENT_TIMESTAMP,
  version INTEGER UNIQUE NOT NULL,
  name TEXT NOT NULL
);
