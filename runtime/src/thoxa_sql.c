#define _DEFAULT_SOURCE
#include "thoxa_rt.h"
#include "sqlite3.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

ThoxaString *thoxa_sql_quote(const ThoxaString *s) {
    const char *src = s && s->data ? s->data : "";
    size_t n = s ? s->len : 0;
    size_t extra = 0;
    for (size_t i = 0; i < n; i++) {
        if (src[i] == '\'') {
            extra++;
        }
    }
    size_t out_len = n + extra + 2;
    char *buf = (char *)malloc(out_len);
    if (!buf) {
        thoxa_panic("out of memory");
    }
    buf[0] = '\'';
    size_t o = 1;
    for (size_t i = 0; i < n; i++) {
        buf[o++] = src[i];
        if (src[i] == '\'') {
            buf[o++] = '\'';
        }
    }
    buf[o++] = '\'';
    ThoxaString *out = thoxa_string_new(buf, out_len);
    free(buf);
    return out;
}

static __thread char last_err[512];

static void set_err(const char *msg) {
    snprintf(last_err, sizeof(last_err), "%s", msg && msg[0] ? msg : "sqlite error");
}

static void set_err_db(sqlite3 *db) {
    set_err(db ? sqlite3_errmsg(db) : "sqlite error");
}

static sqlite3 *as_db(int64_t handle) {
    return (sqlite3 *)(intptr_t)handle;
}

static sqlite3_stmt *as_stmt(int64_t handle) {
    return (sqlite3_stmt *)(intptr_t)handle;
}

const char *thoxa_sql_errmsg(void) {
    return last_err[0] ? last_err : "sqlite error";
}

int64_t thoxa_sql_open(const char *path) {
    if (!path || !path[0]) {
        set_err("empty sqlite path");
        return 0;
    }
    sqlite3 *db = NULL;
    int rc = sqlite3_open(path, &db);
    if (rc != SQLITE_OK) {
        if (db) {
            set_err_db(db);
            sqlite3_close(db);
        } else {
            set_err(sqlite3_errstr(rc));
        }
        return 0;
    }
    return (int64_t)(intptr_t)db;
}

int32_t thoxa_sql_close(int64_t handle) {
    sqlite3 *db = as_db(handle);
    if (!db) {
        set_err("sqlite close: closed handle");
        return -1;
    }
    int rc = sqlite3_close(db);
    if (rc != SQLITE_OK) {
        set_err_db(db);
        return -1;
    }
    return 0;
}

int32_t thoxa_sql_exec(int64_t handle, const char *sql) {
    sqlite3 *db = as_db(handle);
    if (!db) {
        set_err("sqlite exec: closed handle");
        return -1;
    }
    if (!sql) {
        set_err("sqlite exec: empty sql");
        return -1;
    }
    char *errmsg = NULL;
    int rc = sqlite3_exec(db, sql, NULL, NULL, &errmsg);
    if (rc != SQLITE_OK) {
        set_err(errmsg ? errmsg : sqlite3_errmsg(db));
        sqlite3_free(errmsg);
        return -1;
    }
    return 0;
}

int64_t thoxa_sql_last_insert_rowid(int64_t handle) {
    sqlite3 *db = as_db(handle);
    if (!db) {
        return 0;
    }
    return (int64_t)sqlite3_last_insert_rowid(db);
}

int64_t thoxa_sql_changes(int64_t handle) {
    sqlite3 *db = as_db(handle);
    if (!db) {
        return 0;
    }
    return (int64_t)sqlite3_changes64(db);
}

int64_t thoxa_sql_prepare(int64_t handle, const char *sql) {
    sqlite3 *db = as_db(handle);
    if (!db) {
        set_err("sqlite prepare: closed handle");
        return 0;
    }
    if (!sql) {
        set_err("sqlite prepare: empty sql");
        return 0;
    }
    sqlite3_stmt *stmt = NULL;
    int rc = sqlite3_prepare_v2(db, sql, -1, &stmt, NULL);
    if (rc != SQLITE_OK) {
        set_err_db(db);
        if (stmt) {
            sqlite3_finalize(stmt);
        }
        return 0;
    }
    return (int64_t)(intptr_t)stmt;
}

int32_t thoxa_sql_step(int64_t stmt_h) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        set_err("sqlite step: closed statement");
        return -1;
    }
    int rc = sqlite3_step(stmt);
    if (rc != SQLITE_ROW && rc != SQLITE_DONE) {
        set_err(sqlite3_errmsg(sqlite3_db_handle(stmt)));
    }
    return (int32_t)rc;
}

int32_t thoxa_sql_finalize(int64_t stmt_h) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        set_err("sqlite finalize: closed statement");
        return -1;
    }
    sqlite3 *db = sqlite3_db_handle(stmt);
    int rc = sqlite3_finalize(stmt);
    if (rc != SQLITE_OK) {
        set_err_db(db);
        return -1;
    }
    return 0;
}

int32_t thoxa_sql_column_count(int64_t stmt_h) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        return 0;
    }
    return (int32_t)sqlite3_column_count(stmt);
}

const char *thoxa_sql_column_name(int64_t stmt_h, int32_t i) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        return "";
    }
    const char *name = sqlite3_column_name(stmt, i);
    return name ? name : "";
}

int32_t thoxa_sql_column_type(int64_t stmt_h, int32_t i) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        return SQLITE_NULL;
    }
    return (int32_t)sqlite3_column_type(stmt, i);
}

int64_t thoxa_sql_column_i64(int64_t stmt_h, int32_t i) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        return 0;
    }
    return (int64_t)sqlite3_column_int64(stmt, i);
}

double thoxa_sql_column_f64(int64_t stmt_h, int32_t i) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        return 0.0;
    }
    return sqlite3_column_double(stmt, i);
}

const char *thoxa_sql_column_text(int64_t stmt_h, int32_t i) {
    sqlite3_stmt *stmt = as_stmt(stmt_h);
    if (!stmt) {
        return "";
    }
    const unsigned char *text = sqlite3_column_text(stmt, i);
    return text ? (const char *)text : "";
}
