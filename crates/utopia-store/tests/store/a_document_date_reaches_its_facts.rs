//! 文档的日期要跟到它的事实上（#987）。
//!
//! 抽取在定日期之前跑：陈述入库时文档还没有日期，`attested_from` 填的是处理它的那一刻。
//! 日期从正文里认出来之后，引用这篇文档的事实的锚点挪到文档日期——只往早挪；物化出来的
//! 类型化行跟着它的陈述走；别的文档的事实不动；没有带日期证据的事实不动。
//!
//! 没有 `UTOPIA_DATABASE_URL` 时跳过而不是失败。自建自拆，绝不碰已有的库。

use chrono::{DateTime, TimeZone, Utc};
use sqlx::PgPool;
use utopia_store::documents;
use uuid::Uuid;

const ORG: &str = "document-date-reaches-facts-test";

async fn attested(pool: &PgPool, fact: Uuid) -> anyhow::Result<Option<DateTime<Utc>>> {
    Ok(
        sqlx::query_scalar("SELECT attested_from FROM facts WHERE id = $1")
            .bind(fact)
            .fetch_one(pool)
            .await?,
    )
}

/// 一条开放陈述，出处在这篇文档里，锚点是 `at`（处理文档的那一刻）
async fn statement(
    pool: &PgPool,
    kb: Uuid,
    document: Uuid,
    phrase: &str,
    at: DateTime<Utc>,
) -> anyhow::Result<Uuid> {
    let (subject, object, chunk, fact) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    for (id, name) in [(subject, "码表"), (object, "全球")] {
        sqlx::query("INSERT INTO entities (id, kb_id, canonical_name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(kb)
            .bind(name)
            .execute(pool)
            .await?;
    }
    sqlx::query(
        "INSERT INTO chunks (id, kb_id, document_id, seq, text) VALUES ($1, $2, $3, 0, '码表所属市场为全球。')",
    )
    .bind(chunk)
    .bind(kb)
    .bind(document)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO facts (id, kb_id, subject_id, object_id, layer, phrase, attested_from)
         VALUES ($1, $2, $3, $4, 'open', $5, $6)",
    )
    .bind(fact)
    .bind(kb)
    .bind(subject)
    .bind(object)
    .bind(phrase)
    .bind(at)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO fact_evidence (fact_id, chunk_id, quote, document_id)
         VALUES ($1, $2, '码表所属市场为全球', $3)",
    )
    .bind(fact)
    .bind(chunk)
    .bind(document)
    .execute(pool)
    .await?;
    Ok(fact)
}

#[tokio::test]
async fn a_date_read_from_the_text_moves_the_anchor_of_the_facts_that_cite_the_document(
) -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    sqlx::query("DELETE FROM organizations WHERE name = $1")
        .bind(ORG)
        .execute(&pool)
        .await?;
    let (org, ws, kb) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, $2)")
        .bind(org)
        .bind(ORG)
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, $3)")
        .bind(ws)
        .bind(org)
        .bind(ORG)
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, $3)")
        .bind(kb)
        .bind(ws)
        .bind(ORG)
        .execute(&pool)
        .await?;

    let run = async {
        let processed = Utc.with_ymd_and_hms(2026, 9, 28, 4, 26, 0).unwrap();
        let written = Utc.with_ymd_and_hms(2026, 9, 4, 0, 0, 0).unwrap();
        let report = documents::create_from_upload(
            &pool, kb, "报告.md", "text/markdown", 1, "sha-report", None, None,
        )
        .await?;
        let other = documents::create_from_upload(
            &pool, kb, "别的.md", "text/markdown", 1, "sha-other", None, None,
        )
        .await?;
        // 抽取先跑：两篇都还没有日期，锚点是处理它们的那一刻
        let cited = statement(&pool, kb, report.id, "所属市场为", processed).await?;
        let elsewhere = statement(&pool, kb, other.id, "所属市场为", processed).await?;
        // 这条陈述物化出来的类型化行：自己没有出处，锚点抄的是陈述的
        let typed = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO facts (id, kb_id, subject_id, object_id, layer, from_statement_id, attested_from)
             SELECT $1, kb_id, subject_id, object_id, 'typed', id, attested_from FROM facts WHERE id = $2",
        )
        .bind(typed)
        .bind(cited)
        .execute(&pool)
        .await?;
        // 早在文档日期之前就有人说过的：锚点不往晚挪
        let earlier = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let known = statement(&pool, kb, report.id, "所属 BU 为", earlier).await?;

        let moved = documents::set_content_date(&pool, report.id, written).await?;

        assert_eq!(moved, 2, "the statement and the typed row it became");
        assert_eq!(
            attested(&pool, cited).await?,
            Some(written),
            "the statement is attested from the date the document states"
        );
        assert_eq!(
            attested(&pool, typed).await?,
            Some(written),
            "the typed row follows its statement"
        );
        assert_eq!(
            attested(&pool, known).await?,
            Some(earlier),
            "an anchor only moves earlier"
        );
        assert_eq!(
            attested(&pool, elsewhere).await?,
            Some(processed),
            "a fact of another document is not touched"
        );
        // 再认一次同一个日期：没有东西可挪
        assert_eq!(
            documents::set_content_date(&pool, report.id, written).await?,
            0
        );
        // 这一版的日期记在了版本上
        let on_version: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT doc_time FROM document_versions WHERE document_id = $1 ORDER BY version DESC LIMIT 1",
        )
        .bind(report.id)
        .fetch_optional(&pool)
        .await?
        .flatten();
        assert!(on_version.is_none() || on_version == Some(written));
        Ok::<(), anyhow::Error>(())
    }
    .await;

    sqlx::query("DELETE FROM organizations WHERE name = $1")
        .bind(ORG)
        .execute(&pool)
        .await?;
    run
}
