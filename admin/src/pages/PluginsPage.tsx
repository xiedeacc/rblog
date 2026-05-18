import { Button, Card, Space, Switch, Table, Tag, Typography, App } from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  listPlugins,
  enablePlugin,
  disablePlugin,
  reloadPlugins,
  type PluginInfo,
} from "@/api/client";

const { Title, Paragraph, Text } = Typography;

export function PluginsPage() {
  const { message } = App.useApp();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["plugins"], queryFn: listPlugins });

  const toggle = useMutation({
    mutationFn: async (input: { name: string; enable: boolean }) =>
      input.enable ? enablePlugin(input.name) : disablePlugin(input.name),
    onSuccess: () => {
      void message.success("Updated");
      void qc.invalidateQueries({ queryKey: ["plugins"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });

  const reload = useMutation({
    mutationFn: reloadPlugins,
    onSuccess: (res) => {
      void message.success(`Reloaded — ${res.loaded} plugin(s) discovered.`);
      void qc.invalidateQueries({ queryKey: ["plugins"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });

  return (
    <Card>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <Title level={3} style={{ margin: 0 }}>
            Plugins
          </Title>
          <Button
            icon={<ReloadOutlined />}
            loading={reload.isPending}
            onClick={() => reload.mutate()}
          >
            Rescan plugins folder
          </Button>
        </div>
        <Paragraph type="secondary" style={{ margin: 0 }}>
          WASM plugins are discovered under <code>paths.plugins_root</code>.
          Each plugin directory must contain a <code>plugin.toml</code>
          manifest and a <code>plugin.wasm</code> binary. The host runtime
          enforces declared capabilities at every host-call.
        </Paragraph>
        <Table<PluginInfo>
          rowKey="name"
          loading={query.isLoading}
          dataSource={query.data ?? []}
          pagination={false}
          expandable={{
            expandedRowRender: (row) => (
              <Space direction="vertical" size="small" style={{ width: "100%" }}>
                {row.description && <Paragraph>{row.description}</Paragraph>}
                <Text strong>Directory</Text>
                <code>{row.directory}</code>
                <Text strong>Entry</Text>
                <code>{row.entry}</code>
                <Text strong>Routes</Text>
                {row.routes.length === 0 ? (
                  <Text type="secondary">none</Text>
                ) : (
                  <ul>
                    {row.routes.map((r) => (
                      <li key={r.path}>
                        <code>
                          /api/plugins/{row.name}
                          {r.path}
                        </code>{" "}
                        ({r.methods.join(", ")})
                      </li>
                    ))}
                  </ul>
                )}
              </Space>
            ),
          }}
          columns={[
            {
              title: "Name",
              key: "name",
              render: (_v, row) => (
                <Space direction="vertical" size={0}>
                  <Text strong>{row.display_name}</Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {row.name}@{row.version}
                  </Text>
                </Space>
              ),
            },
            {
              title: "Capabilities",
              dataIndex: "capabilities",
              render: (caps: string[]) =>
                caps.length === 0 ? (
                  <Text type="secondary">none</Text>
                ) : (
                  caps.map((c) => <Tag key={c}>{c}</Tag>)
                ),
            },
            {
              title: "Status",
              key: "status",
              render: (_v, row) =>
                row.enabled ? (
                  <Tag color="green">enabled</Tag>
                ) : (
                  <Tag>disabled</Tag>
                ),
            },
            {
              title: "Actions",
              key: "actions",
              render: (_v, row) => (
                <Switch
                  checked={row.enabled}
                  loading={toggle.isPending}
                  onChange={(v) => toggle.mutate({ name: row.name, enable: v })}
                />
              ),
            },
          ]}
        />
      </Space>
    </Card>
  );
}
