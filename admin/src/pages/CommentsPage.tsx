import { Button, Card, Empty, Popconfirm, Radio, Space, Tag, Typography, App } from "antd";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import {
  listComments,
  approveComment,
  approveReply,
  hideComment,
  hideReply,
  showComment,
  showReply,
  deleteComment,
  deleteReply,
} from "@/api/client";

const { Title, Text } = Typography;
type StatusFilter = "all" | "pending" | "approved" | "hidden";
type KindFilter = "all" | "Comment" | "Reply";

export function CommentsPage() {
  const { message } = App.useApp();
  const qc = useQueryClient();
  const [status, setStatus] = useState<StatusFilter>("all");
  const [kind, setKind] = useState<KindFilter>("all");
  const params = useMemo(
    () => ({
      status,
      kind: kind === "all" ? undefined : kind,
    }),
    [kind, status],
  );
  const query = useQuery({
    queryKey: ["comments", params],
    queryFn: () => listComments(params),
  });

  const act = useMutation({
    mutationFn: async (input: { kind: "approve" | "hide" | "show" | "delete"; name: string }) => {
      const item = query.data?.find((row) => row.name === input.name);
      const isReply = item?.kind === "Reply";
      if (input.kind === "approve") return isReply ? approveReply(input.name) : approveComment(input.name);
      if (input.kind === "hide") return isReply ? hideReply(input.name) : hideComment(input.name);
      if (input.kind === "show") return isReply ? showReply(input.name) : showComment(input.name);
      return isReply ? deleteReply(input.name) : deleteComment(input.name);
    },
    onSuccess: () => {
      void message.success("Done");
      void qc.invalidateQueries({ queryKey: ["comments"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });
  const comments = query.data ?? [];
  const pendingCount = comments.filter((row) => !row.approved && !row.hidden).length;

  return (
    <div className="halo-comments-page">
      <div className="halo-page-header">
        <div>
          <Text type="secondary">Interactions</Text>
          <Title level={2} style={{ margin: 0 }}>
            Comments
          </Title>
        </div>
        <Tag color={pendingCount > 0 ? "gold" : "green"}>{pendingCount} pending</Tag>
      </div>

      <Card className="halo-comments-toolbar">
        <Space wrap>
          <Radio.Group value={status} onChange={(event) => setStatus(event.target.value)}>
            <Radio.Button value="all">All</Radio.Button>
            <Radio.Button value="pending">Pending</Radio.Button>
            <Radio.Button value="approved">Approved</Radio.Button>
            <Radio.Button value="hidden">Hidden</Radio.Button>
          </Radio.Group>
          <Radio.Group value={kind} onChange={(event) => setKind(event.target.value)}>
            <Radio.Button value="all">Comments and replies</Radio.Button>
            <Radio.Button value="Comment">Comments</Radio.Button>
            <Radio.Button value="Reply">Replies</Radio.Button>
          </Radio.Group>
        </Space>
      </Card>

      <div className="halo-comments-list" aria-busy={query.isLoading}>
        {comments.length === 0 ? (
          <Card>
            <Empty description={query.isLoading ? "Loading comments..." : "No comments"} />
          </Card>
        ) : (
          comments.map((row) => (
            <article className="halo-comment-row" key={row.name}>
              <div className="halo-comment-avatar">{row.owner_display.slice(0, 1).toUpperCase()}</div>
              <div className="halo-comment-main">
                <header className="halo-comment-header">
                  <Space size="small" wrap>
                    <Text strong>{row.owner_display}</Text>
                    <Text type="secondary">{row.owner_kind}:{row.owner_name}</Text>
                    <Tag>{row.kind === "Reply" ? "reply" : "comment"}</Tag>
                    {row.hidden ? <Tag>hidden</Tag> : row.approved ? <Tag color="green">approved</Tag> : <Tag color="gold">pending</Tag>}
                  </Space>
                  <Text type="secondary">{row.created_at ?? ""}</Text>
                </header>
                <div className="halo-comment-target">
                  {row.kind === "Reply" ? `Reply to comment ${row.parent_name}` : `${row.subject_kind}:${row.subject_name}`}
                </div>
                <div className="halo-comment-content" dangerouslySetInnerHTML={{ __html: row.content || row.raw }} />
                <Space className="halo-comment-actions">
                  {!row.approved && (
                    <Button size="small" type="primary" onClick={() => act.mutate({ kind: "approve", name: row.name })}>
                      Approve
                    </Button>
                  )}
                  {!row.hidden && (
                    <Button size="small" onClick={() => act.mutate({ kind: "hide", name: row.name })}>
                      Hide
                    </Button>
                  )}
                  {row.hidden && (
                    <Button size="small" onClick={() => act.mutate({ kind: "show", name: row.name })}>
                      Show
                    </Button>
                  )}
                  <Popconfirm title="Delete comment permanently?" onConfirm={() => act.mutate({ kind: "delete", name: row.name })}>
                    <Button size="small" danger>
                      Delete
                    </Button>
                  </Popconfirm>
                </Space>
              </div>
            </article>
          ))
        )}
      </div>
    </div>
  );
}
