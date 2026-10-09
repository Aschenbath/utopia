/** 推送来源的接口地址。
 *
 * 两种推送来源各有各的端点：`api` 收文档走 `/ingest`，`statements` 收陈述走
 * `/statements`（0054）。服务端对走错门的一律回 404——令牌弹窗从前对两种都写
 * `/ingest`，照着弹窗推陈述的人拿到的就是一个没有解释的 Not found（#924）。 */
export function pushEndpoint(origin: string, sourceId: string, kind: string): string {
  const door = kind === "statements" ? "statements" : "ingest";
  return `${origin}/api/v1/sources/${sourceId}/${door}`;
}
