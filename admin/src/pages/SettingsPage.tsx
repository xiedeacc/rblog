import { useEffect, useState } from "react";
import { Alert, Button, Card, Form, Input, Space, Typography, App, Tabs } from "antd";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  fetchSystemSettings,
  upsertSystemSettings,
  fetchSystemInfo,
  restoreHaloDump,
  type RestoreHaloDumpResponse,
} from "@/api/client";

const { Title, Paragraph } = Typography;

export function SettingsPage() {
  const { message } = App.useApp();
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["settings-system"], queryFn: fetchSystemSettings });
  const info = useQuery({ queryKey: ["system-info"], queryFn: fetchSystemInfo });
  const [form] = Form.useForm();
  const [restoreForm] = Form.useForm<{ path: string }>();
  const [keys, setKeys] = useState<{ key: string; value: string }[]>([]);
  const [restoreResult, setRestoreResult] = useState<RestoreHaloDumpResponse | null>(null);

  useEffect(() => {
    if (settings.data) {
      const entries = Object.entries(settings.data.data).map(([key, value]) => ({ key, value }));
      setKeys(entries);
      form.setFieldsValue(Object.fromEntries(entries.map((e) => [e.key, e.value])));
    }
  }, [settings.data, form]);

  const save = useMutation({
    mutationFn: (vals: Record<string, string>) => upsertSystemSettings(vals),
    onSuccess: () => {
      void message.success("Saved");
      void qc.invalidateQueries({ queryKey: ["settings-system"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });

  const restore = useMutation({
    mutationFn: ({ path }: { path: string }) => restoreHaloDump(path),
    onSuccess: (result) => {
      setRestoreResult(result);
      void message.success(`Restored ${result.restored_rows} rows`);
      void qc.invalidateQueries();
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Restore failed"),
  });

  return (
    <Card>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Title level={3} style={{ margin: 0 }}>
          Settings
        </Title>

        <Tabs
          defaultActiveKey="site"
          items={[
            {
              key: "site",
              label: "Site",
              children: (
                <Form form={form} layout="vertical" onFinish={(v) => save.mutate(v)}>
                  {keys.map((k) => (
                    <Form.Item key={k.key} name={k.key} label={k.key}>
                      <Input />
                    </Form.Item>
                  ))}
                  <Form.Item>
                    <Button type="primary" htmlType="submit" loading={save.isPending}>
                      Save
                    </Button>
                  </Form.Item>
                </Form>
              ),
            },
            {
              key: "theme",
              label: "Theme",
              children: (
                <Space direction="vertical">
                  <Paragraph>
                    Active theme: <strong>{info.data?.active_theme ?? "(loading)"}</strong>
                  </Paragraph>
                  <Paragraph type="secondary">
                    Installed themes: {info.data?.themes.join(", ")}
                  </Paragraph>
                  <Paragraph type="secondary">
                    Theme directory: <code>{info.data?.active_theme_directory}</code>
                  </Paragraph>
                </Space>
              ),
            },
            {
              key: "restore",
              label: "Restore",
              children: (
                <Space direction="vertical" size="middle" style={{ width: "100%" }}>
                  <Alert
                    type="warning"
                    showIcon
                    message="Restore replaces all current content"
                    description="Use this to import a Halo MySQL dump into the active rblog database, then rebuild the in-memory and search indexes."
                  />
                  <Form
                    form={restoreForm}
                    layout="vertical"
                    initialValues={{ path: "/root/src/blog/db/halodb_backup.sql" }}
                    onFinish={(values) => restore.mutate(values)}
                  >
                    <Form.Item
                      name="path"
                      label="Server dump path"
                      rules={[{ required: true, message: "Enter a dump path" }]}
                    >
                      <Input />
                    </Form.Item>
                    <Form.Item>
                      <Button danger type="primary" htmlType="submit" loading={restore.isPending}>
                        Restore Halo dump
                      </Button>
                    </Form.Item>
                  </Form>
                  {restoreResult ? (
                    <Paragraph type="secondary">
                      Restored {restoreResult.restored_rows} rows: {restoreResult.posts} posts,{" "}
                      {restoreResult.snapshots} snapshots, {restoreResult.categories} categories,{" "}
                      {restoreResult.tags} tags, {restoreResult.comments} comments,{" "}
                      {restoreResult.users} users. Search indexed {restoreResult.search_indexed} posts.
                    </Paragraph>
                  ) : null}
                </Space>
              ),
            },
          ]}
        />
      </Space>
    </Card>
  );
}
