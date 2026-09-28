-- #987 · 文档的日期要跟到它的事实上：已经写下的那些。
--
-- 抽取在定日期之前跑。陈述入库时文档还没有日期，`attested_from` 填的是处理文档的那一刻；
-- 日期后来从正文里认出来，只写在了文档上。读的人拿没起点的事实的下界时用的是
-- `COALESCE(valid_from, attested_from)`，于是一份 2020 年的报告今天传上来，它的事实读作
-- 「今天才有」——#714 说上传时刻不许进时间，它从这条缝里进来了。
--
-- 写入口已经补上（`documents::set_content_date` 认出日期时把引用这篇文档的事实一并挪过去）。
-- 这里补已有的行：锚点挪到最早的带日期证据上，只往早挪；口径同 `temporal::DATED_AT`
-- （日期来自正文或来源系统才算，证据所在那一版的日期优先）。类型化的行自己没有出处，
-- 跟着物化它的那条陈述走。引擎画的终点不在这里重算：它们本来就按证据的日期排，
-- 时间线下一次变动时照旧整条重算。
UPDATE facts f
   SET attested_from = LEAST(f.attested_from, e.first),
       attested_to = CASE WHEN f.attested_to IS NOT NULL THEN LEAST(f.attested_to, e.first) END
  FROM (SELECT fe.fact_id, min(COALESCE(v.doc_time, d.doc_time)) AS first
          FROM fact_evidence fe
          JOIN documents d ON d.id = fe.document_id
     LEFT JOIN document_versions v ON v.document_id = fe.document_id
                                  AND v.version = fe.doc_version
         WHERE d.deleted_at IS NULL
           AND d.doc_time_source IN ('content', 'source')
           AND COALESCE(v.doc_time, d.doc_time) IS NOT NULL
         GROUP BY fe.fact_id) e
 WHERE f.id = e.fact_id AND f.invalidated_at IS NULL
   AND (f.attested_from IS NULL OR f.attested_from > e.first OR f.attested_to > e.first);

UPDATE facts t
   SET attested_from = LEAST(t.attested_from, s.attested_from),
       attested_to = CASE WHEN t.attested_to IS NOT NULL
                          THEN LEAST(t.attested_to, s.attested_from) END
  FROM facts s
 WHERE t.from_statement_id = s.id AND t.layer = 'typed'
   AND t.invalidated_at IS NULL AND s.invalidated_at IS NULL
   AND (t.attested_from IS NULL OR t.attested_from > s.attested_from
        OR t.attested_to > s.attested_from);
