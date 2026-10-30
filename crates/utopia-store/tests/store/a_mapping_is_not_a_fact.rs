//! 语义层映射的两条硬性质——打在真库上（见 `docs/decisions/0011`）。
//!
//! 这两条都是搬出账本换来的东西，也正是从前做不到的：
//!
//! 1. **同一个 (概念, 源) 只有一条。** 从前这条唯一性藏在 `object_value`
//!    这个 JSONB 内部，数据库看不见，只能靠确认流程显式闭合——也就是靠流程
//!    而不是约束。现在它是主键。
//! 2. **表过态的不被下一轮探索刷回待看。** `ontology_proposals` 那边踩过同一个
//!    坑（`ontology_proposals` 那边）：重跑必然再次算出被拒绝过的那条，不挡住就等于每跑一次都把
//!    人的否决抹掉一次。

use sqlx::PgPool;
use uuid::Uuid;

async fn fixture(pool: &PgPool) -> anyhow::Result<(Uuid, Uuid, Uuid)> {
    let (org, ws, kb, ent) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'map-test')")
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, org_id, name) VALUES ($1, $2, 'map-test')")
        .bind(ws)
        .bind(org)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO knowledge_bases (id, workspace_id, name) VALUES ($1, $2, 'map-test')")
        .bind(kb)
        .bind(ws)
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO entities (id, kb_id, canonical_name) VALUES ($1, $2, '营收')")
        .bind(ent)
        .bind(kb)
        .execute(pool)
        .await?;
    Ok((org, kb, ent))
}

#[tokio::test]
async fn one_concept_one_source_one_mapping() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let (_org, kb, ent) = fixture(&pool).await?;

    let run = async {
        let p = |t: &'static str| {
            utopia_store::mappings::propose(
                &pool,
                kb,
                ent,
                "warehouse",
                Some(t),
                None,
                None,
                None,
                None,
                false,
            )
        };
        let (a, wrote_a) = p("orders").await?;
        let (b, wrote_b) = p("orders_v2").await?;
        assert_eq!(a, b, "同一个 (概念, 源) 该是同一行，不是两行");
        assert!(wrote_a && wrote_b, "第一次插入、第二次刷新，都算写入");

        let got = utopia_store::mappings::proposed(&pool, kb, 100, 0).await?;
        assert_eq!(got.len(), 1, "只该有一条");
        assert_eq!(
            got[0].table_name.as_deref(),
            Some("orders_v2"),
            "重跑探索该刷新定义"
        );

        // 换一个源就是另一条：同一概念在不同源上有不同定义是有意支持的
        utopia_store::mappings::propose(
            &pool,
            kb,
            ent,
            "lakehouse",
            Some("f_orders"),
            None,
            None,
            None,
            None,
            false,
        )
        .await?;
        assert_eq!(
            utopia_store::mappings::proposed(&pool, kb, 100, 0)
                .await?
                .len(),
            2,
            "不同源该各有一条"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    // **只删知识库,不删 org/user。** 用户是软删除的,生产代码里没有
    // `DELETE FROM users`——测试也不该造一个产品里不存在的动作,否则
    // 撞上的外键约束是夹具自己的问题,会被误当成产品缺陷（实测发生过）
    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(kb)
        .execute(&pool)
        .await?;
    run
}

#[tokio::test]
async fn a_rejected_mapping_does_not_come_back() -> anyhow::Result<()> {
    let Some(url) = utopia_store::test_db::url() else {
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    let (org, kb, ent) = fixture(&pool).await?;

    let run = async {
        let user = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO users (id, org_id, email, display_name, password_hash)
             VALUES ($1, $2, $1 || '@m.test', 'm', 'x')",
        )
        .bind(user)
        .bind(org)
        .execute(&pool)
        .await?;

        let (id, _) = utopia_store::mappings::propose(
            &pool,
            kb,
            ent,
            "warehouse",
            Some("orders"),
            None,
            None,
            None,
            None,
            false,
        )
        .await?;
        utopia_store::mappings::decide(&pool, kb, id, "rejected", user).await?;

        // 下一轮探索会再次算出同一条——它不该被刷回待看，也不算写入
        let (_, written) = utopia_store::mappings::propose(
            &pool,
            kb,
            ent,
            "warehouse",
            Some("orders"),
            None,
            None,
            None,
            None,
            false,
        )
        .await?;
        assert!(!written, "撞上决定的提议不算写入，探索账把它记成 decided");
        assert!(
            utopia_store::mappings::proposed(&pool, kb, 100, 0)
                .await?
                .is_empty(),
            "拒绝过的不该重新排队"
        );
        // 确认过的同理不该被探索覆盖
        assert!(
            utopia_store::mappings::confirmed(&pool, kb, 100)
                .await?
                .is_empty(),
            "拒绝的也不该出现在确认列表里"
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;

    // **只删知识库,不删 org/user。** 用户是软删除的,生产代码里没有
    // `DELETE FROM users`——测试也不该造一个产品里不存在的动作,否则
    // 撞上的外键约束是夹具自己的问题,会被误当成产品缺陷（实测发生过）
    sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
        .bind(kb)
        .execute(&pool)
        .await?;
    run
}

/// 一个打开了自动确认的库，给下面每条自动确认的性质各用一份。
struct Auto {
    pool: PgPool,
    org: Uuid,
    kb: Uuid,
    ent: Uuid,
}

impl Auto {
    async fn new() -> anyhow::Result<Option<Self>> {
        let Some(url) = utopia_store::test_db::url() else {
            return Ok(None);
        };
        let pool = PgPool::connect(&url).await?;
        utopia_store::db::migrate(&pool).await?;
        let (org, kb, ent) = fixture(&pool).await?;
        let auto = Self { pool, org, kb, ent };
        auto.switch(true).await?;
        Ok(Some(auto))
    }

    async fn switch(&self, on: bool) -> anyhow::Result<()> {
        sqlx::query("UPDATE knowledge_bases SET auto_confirm_mappings = $2 WHERE id = $1")
            .bind(self.kb)
            .bind(on)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn person(&self) -> anyhow::Result<Uuid> {
        let user = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO users (id, org_id, email, display_name, password_hash)
             VALUES ($1, $2, $1 || '@auto-map.test', 'Editor', 'unused')",
        )
        .bind(user)
        .bind(self.org)
        .execute(&self.pool)
        .await?;
        Ok(user)
    }

    async fn propose(
        &self,
        confidence: Option<f64>,
        rationale: Option<&str>,
        expr: Option<&str>,
    ) -> utopia_core::AppResult<(Uuid, bool)> {
        utopia_store::mappings::propose_with_evidence(
            &self.pool,
            self.kb,
            self.ent,
            "warehouse",
            Some("orders"),
            expr,
            None,
            None,
            Some("Revenue"),
            false,
            confidence,
            rationale,
        )
        .await
    }

    /// 一条过得了门槛的提议：开关开着它就直接生效。
    async fn grounded(&self, expr: &str) -> anyhow::Result<(Uuid, bool)> {
        Ok(self
            .propose(Some(0.9), Some("Grounded"), Some(expr))
            .await?)
    }

    async fn effective(&self) -> anyhow::Result<Vec<utopia_core::models::ConceptMapping>> {
        Ok(utopia_store::mappings::confirmed(&self.pool, self.kb, 10).await?)
    }

    async fn pending(&self) -> anyhow::Result<Vec<utopia_core::models::ConceptMapping>> {
        Ok(utopia_store::mappings::proposed(&self.pool, self.kb, 10, 0).await?)
    }

    async fn events(&self, action: &str) -> anyhow::Result<Vec<(Option<Uuid>, serde_json::Value)>> {
        Ok(sqlx::query_as(
            "SELECT actor_id, detail FROM audit_events WHERE kb_id = $1 AND action = $2",
        )
        .bind(self.kb)
        .bind(action)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM knowledge_bases WHERE id = $1")
            .bind(self.kb)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[tokio::test]
async fn a_base_that_has_not_opted_in_only_collects_proposals() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    a.switch(false).await?;
    a.propose(
        Some(1.0),
        Some("The schema states revenue"),
        Some("SUM(amount)"),
    )
    .await?;
    assert!(a.effective().await?.is_empty());
    assert_eq!(a.pending().await?.len(), 1);
    a.cleanup().await
}

#[tokio::test]
async fn missing_or_invalid_evidence_leaves_a_proposal_waiting() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    for (confidence, rationale, expr) in [
        (None, Some("Grounded"), Some("SUM(amount)")),
        (Some(0.79), Some("Grounded"), Some("SUM(amount)")),
        (Some(f64::NAN), Some("Grounded"), Some("SUM(amount)")),
        (Some(1.1), Some("Grounded"), Some("SUM(amount)")),
        (Some(0.9), Some("  "), Some("SUM(amount)")),
        (Some(0.9), Some("Grounded"), None),
    ] {
        a.propose(confidence, rationale, expr).await?;
        assert!(a.effective().await?.is_empty());
    }
    a.cleanup().await
}

#[tokio::test]
async fn the_threshold_itself_confirms_and_the_readers_show_its_evidence() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let (id, written) = a
        .propose(
            Some(0.8),
            Some("  The schema states revenue  "),
            Some("SUM(amount)"),
        )
        .await?;
    assert!(written);
    let auto = a.effective().await?.remove(0);
    assert_eq!(auto.confidence, Some(0.8));
    assert_eq!(auto.rationale.as_deref(), Some("The schema states revenue"));
    assert_eq!(auto.decided_by, None);
    assert!(auto.auto_confirmed_at.is_some());
    let page = utopia_store::mappings::page(&a.pool, a.kb, None, None, 10, 0)
        .await?
        .0;
    assert_eq!(page[0].rationale, auto.rationale);
    let by_id = utopia_store::mappings::by_ids(&a.pool, a.kb, &[id]).await?;
    assert_eq!(by_id[0].confidence, Some(0.8));
    a.cleanup().await
}

#[tokio::test]
async fn an_automatic_write_is_audited_with_no_actor_and_its_definition() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    a.grounded("SUM(amount)").await?;
    let events = a.events("mapping.auto_confirmed").await?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, None);
    assert_eq!(events[0].1["definition"]["expr"], "SUM(amount)");
    assert_eq!(events[0].1["rationale"], "Grounded");
    a.cleanup().await
}

#[tokio::test]
async fn a_failed_audit_undoes_the_automatic_write() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    a.propose(None, None, None).await?;
    // 触发器只认这个库，别的测试可以同时跑
    let (kb, fault) = (a.kb, format!("auto_mapping_fault_{}", a.kb.simple()));
    sqlx::raw_sql(&format!(
        "CREATE FUNCTION {fault}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
           IF NEW.kb_id = '{kb}' AND NEW.action = 'mapping.auto_confirmed' THEN
             RAISE EXCEPTION 'injected mapping audit failure'; END IF; RETURN NEW; END $$;
         CREATE TRIGGER {fault} BEFORE INSERT ON audit_events
           FOR EACH ROW EXECUTE FUNCTION {fault}();"
    ))
    .execute(&a.pool)
    .await?;
    let failed = a.grounded("SUM(amount)").await;
    sqlx::raw_sql(&format!(
        "DROP TRIGGER {fault} ON audit_events; DROP FUNCTION {fault}();"
    ))
    .execute(&a.pool)
    .await?;
    assert!(failed.is_err());
    assert!(a.effective().await?.is_empty());
    assert_eq!(a.pending().await?[0].expr, None);
    a.cleanup().await
}

#[tokio::test]
async fn a_better_definition_refreshes_an_automatic_one_and_keeps_the_old() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let (id, _) = a.grounded("SUM(amount)").await?;
    assert_eq!(a.grounded("SUM(net_amount)").await?, (id, true));
    assert_eq!(
        a.effective().await?[0].expr.as_deref(),
        Some("SUM(net_amount)")
    );
    let revisions = utopia_store::mappings::revisions(&a.pool, a.kb, id).await?;
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].before["expr"], "SUM(amount)");
    assert_eq!(revisions[0].changed_by_name, None);
    a.cleanup().await
}

#[tokio::test]
async fn the_same_definition_again_adds_no_version() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let (id, _) = a.grounded("SUM(amount)").await?;
    a.grounded("SUM(amount)").await?;
    assert!(utopia_store::mappings::revisions(&a.pool, a.kb, id)
        .await?
        .is_empty());
    a.cleanup().await
}

#[tokio::test]
async fn weaker_evidence_does_not_demote_an_automatic_definition() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let (id, _) = a.grounded("SUM(amount)").await?;
    let result = a
        .propose(Some(0.7), Some("Uncertain"), Some("SUM(other)"))
        .await?;
    assert_eq!(result, (id, false));
    let kept = a.effective().await?.remove(0);
    assert_eq!(kept.expr.as_deref(), Some("SUM(amount)"));
    assert_eq!(kept.confidence, Some(0.9));
    a.cleanup().await
}

#[tokio::test]
async fn turning_the_switch_off_neither_revokes_nor_refreshes() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let (id, _) = a.grounded("SUM(amount)").await?;
    a.switch(false).await?;
    assert_eq!(
        a.effective().await?.len(),
        1,
        "the switch alone revokes nothing"
    );
    assert_eq!(a.grounded("SUM(other)").await?, (id, false));
    assert_eq!(a.effective().await?[0].expr.as_deref(), Some("SUM(amount)"));
    a.cleanup().await
}

/// `scripts/bench/ask.mjs` 就是这样写真值的：已确认、没有 decided_by。开关开不开都不能动它。
#[tokio::test]
async fn exploration_leaves_a_confirmed_row_it_did_not_confirm() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let (id, _) = a.propose(None, None, Some("SUM(truth)")).await?;
    sqlx::query("UPDATE concept_mappings SET status = 'confirmed' WHERE id = $1")
        .bind(id)
        .execute(&a.pool)
        .await?;
    assert_eq!(a.grounded("SUM(amount)").await?, (id, false));
    a.switch(false).await?;
    assert_eq!(a.grounded("SUM(amount)").await?, (id, false));
    assert_eq!(a.effective().await?[0].expr.as_deref(), Some("SUM(truth)"));
    a.cleanup().await
}

#[tokio::test]
async fn editing_a_pending_proposal_keeps_it_from_exploration() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let user = a.person().await?;
    let (id, _) = a
        .propose(Some(0.5), Some("Unsure"), Some("SUM(amount)"))
        .await?;
    utopia_store::mappings::revise(
        &a.pool,
        a.kb,
        id,
        Some("orders"),
        Some("SUM(human_amount)"),
        None,
        None,
        None,
        false,
        user,
    )
    .await?;
    assert_eq!(a.grounded("SUM(amount)").await?, (id, false));
    let human = a.pending().await?.remove(0);
    assert_eq!(human.expr.as_deref(), Some("SUM(human_amount)"));
    assert_eq!(human.decided_by, Some(user));
    a.cleanup().await
}

#[tokio::test]
async fn editing_clears_the_evidence_of_the_preceding_definition() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let user = a.person().await?;
    let (id, _) = a.grounded("SUM(amount)").await?;
    utopia_store::mappings::revise(
        &a.pool,
        a.kb,
        id,
        Some("orders"),
        Some("SUM(human_amount)"),
        None,
        None,
        None,
        false,
        user,
    )
    .await?;
    let edited = a.effective().await?.remove(0);
    assert_eq!(edited.confidence, None);
    assert_eq!(edited.rationale, None);
    assert_eq!(edited.auto_confirmed_at, None);
    let revisions = utopia_store::mappings::revisions(&a.pool, a.kb, id).await?;
    assert_eq!(revisions[0].before["confidence"], 0.9);
    a.cleanup().await
}

#[tokio::test]
async fn a_persons_decision_ends_the_automation_on_that_row() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let user = a.person().await?;
    let (id, _) = a.grounded("SUM(amount)").await?;
    for status in ["confirmed", "rejected"] {
        utopia_store::mappings::decide(&a.pool, a.kb, id, status, user).await?;
        assert_eq!(a.grounded("SUM(other)").await?, (id, false));
        let row = utopia_store::mappings::by_ids(&a.pool, a.kb, &[id])
            .await?
            .remove(0);
        assert_eq!(row.status, status);
        assert_eq!(row.expr.as_deref(), Some("SUM(amount)"));
        assert_eq!(row.auto_confirmed_at, None);
    }
    a.cleanup().await
}

#[tokio::test]
async fn a_revert_takes_the_definition_out_of_effect_and_records_who() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let user = a.person().await?;
    let (id, _) = a.grounded("SUM(amount)").await?;
    utopia_store::mappings::revert_automatic(&a.pool, a.kb, id, user).await?;
    assert!(a.effective().await?.is_empty());
    let back = a.pending().await?.remove(0);
    assert_eq!(back.expr.as_deref(), Some("SUM(amount)"));
    assert_eq!(back.decided_by, Some(user));
    assert_eq!(back.auto_confirmed_at, None);
    let events = a.events("mapping.auto_confirm_reverted").await?;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, Some(user));
    assert_eq!(events[0].1["before"]["expr"], "SUM(amount)");
    assert_eq!(
        utopia_store::mappings::revisions(&a.pool, a.kb, id)
            .await?
            .len(),
        1
    );
    a.cleanup().await
}

#[tokio::test]
async fn exploration_does_not_confirm_a_reverted_definition_again() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let user = a.person().await?;
    let (id, _) = a.grounded("SUM(amount)").await?;
    utopia_store::mappings::revert_automatic(&a.pool, a.kb, id, user).await?;
    assert_eq!(a.grounded("SUM(amount)").await?, (id, false));
    assert!(a.effective().await?.is_empty());
    a.cleanup().await
}

#[tokio::test]
async fn a_person_can_still_confirm_a_reverted_definition() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let user = a.person().await?;
    let (id, _) = a.grounded("SUM(amount)").await?;
    utopia_store::mappings::revert_automatic(&a.pool, a.kb, id, user).await?;
    utopia_store::mappings::decide(&a.pool, a.kb, id, "confirmed", user).await?;
    assert_eq!(a.effective().await?.len(), 1);
    a.cleanup().await
}

#[tokio::test]
async fn only_an_automatic_confirmation_can_be_reverted() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let user = a.person().await?;
    let (id, _) = a.grounded("SUM(amount)").await?;
    utopia_store::mappings::decide(&a.pool, a.kb, id, "confirmed", user).await?;
    let refused = utopia_store::mappings::revert_automatic(&a.pool, a.kb, id, user).await;
    assert!(matches!(refused, Err(utopia_core::AppError::Conflict(_))));
    assert_eq!(a.effective().await?[0].decided_by, Some(user));
    let missing =
        utopia_store::mappings::revert_automatic(&a.pool, a.kb, Uuid::now_v7(), user).await;
    assert!(matches!(missing, Err(utopia_core::AppError::NotFound)));
    a.cleanup().await
}

#[tokio::test]
async fn concurrent_automatic_proposals_return_the_stored_id() -> anyhow::Result<()> {
    let Some(a) = Auto::new().await? else {
        return Ok(());
    };
    let (x, y) = tokio::join!(a.grounded("COUNT(*)"), a.grounded("COUNT(*)"));
    let (x, y) = (x?, y?);
    assert_eq!(x, y);
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT target_id FROM audit_events WHERE kb_id = $1 AND action = 'mapping.auto_confirmed'",
    )
    .bind(a.kb)
    .fetch_all(&a.pool)
    .await?;
    assert_eq!(ids, vec![x.0, x.0]);
    a.cleanup().await
}
