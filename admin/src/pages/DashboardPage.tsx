import { useQuery } from "@tanstack/react-query";
import { Card, Col, Row, Statistic, Typography, Button, Space, App } from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { fetchSystemInfo, listPosts, listCommentQueue, rebuildSearchIndex } from "@/api/client";

const { Title, Paragraph } = Typography;

export function DashboardPage() {
  const { message } = App.useApp();
  const info = useQuery({ queryKey: ["system-info"], queryFn: fetchSystemInfo });
  const posts = useQuery({ queryKey: ["posts-summary"], queryFn: () => listPosts({ limit: 1 }) });
  const pending = useQuery({
    queryKey: ["comments-pending"],
    queryFn: listCommentQueue,
  });

  const onRebuild = async () => {
    try {
      const res = await rebuildSearchIndex();
      void message.success(`Search index rebuilt (${res.indexed} documents).`);
    } catch (e) {
      void message.error(e instanceof Error ? e.message : "Rebuild failed");
    }
  };

  return (
    <Space direction="vertical" size="large" style={{ width: "100%" }}>
      <div>
        <Title level={2} style={{ margin: 0 }}>
          Dashboard
        </Title>
        <Paragraph type="secondary">
          Welcome to rblog. Theme:{" "}
          <strong>{info.data?.active_theme ?? "(loading)"}</strong> · Version{" "}
          <strong>{info.data?.version ?? "?"}</strong>
        </Paragraph>
      </div>
      <Row gutter={16}>
        <Col xs={24} sm={12} md={8}>
          <Card>
            <Statistic title="Posts" value={posts.data?.total ?? 0} />
          </Card>
        </Col>
        <Col xs={24} sm={12} md={8}>
          <Card>
            <Statistic title="Themes installed" value={info.data?.themes.length ?? 0} />
          </Card>
        </Col>
        <Col xs={24} sm={12} md={8}>
          <Card>
            <Statistic
              title="Pending comments"
              value={pending.data?.length ?? 0}
              valueStyle={
                (pending.data?.length ?? 0) > 0 ? { color: "#cf1322" } : undefined
              }
            />
          </Card>
        </Col>
      </Row>
      <Card title="Operations">
        <Space>
          <Button icon={<ReloadOutlined />} onClick={onRebuild}>
            Rebuild search index
          </Button>
        </Space>
      </Card>
    </Space>
  );
}
