import { useState } from "react";
import { Card, Form, Input, Button, Typography, App, Space } from "antd";
import { useNavigate } from "@tanstack/react-router";
import { bootstrap, login, fetchWhoAmI } from "@/api/client";
import { useAuthStore } from "@/state/auth";

const { Title, Paragraph } = Typography;

export function BootstrapPage() {
  const { message } = App.useApp();
  const navigate = useNavigate();
  const setUser = useAuthStore((s) => s.setUser);
  const [loading, setLoading] = useState(false);

  const onFinish = async (values: {
    admin_username: string;
    admin_email?: string;
    admin_password: string;
    site_title?: string;
    site_subtitle?: string;
    site_base_url?: string;
  }) => {
    setLoading(true);
    try {
      await bootstrap(values);
      await login(values.admin_username, values.admin_password);
      const user = await fetchWhoAmI();
      setUser(user);
      void message.success("rblog is ready. Welcome!");
      navigate({ to: "/" });
    } catch (e) {
      void message.error(
        e instanceof Error ? e.message : "Bootstrap failed. Inspect logs.",
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="full-page-center">
      <Card style={{ maxWidth: 480, width: "100%" }}>
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Title level={3} style={{ margin: 0 }}>
            Set up the administrator
          </Title>
          <Paragraph type="secondary" style={{ margin: 0 }}>
            Choose the first admin username and password. This setup is
            available only until an admin user exists.
          </Paragraph>
          <Form layout="vertical" onFinish={onFinish}>
            <Form.Item
              label="Admin username"
              name="admin_username"
              rules={[{ required: true }]}
            >
              <Input autoFocus autoComplete="username" />
            </Form.Item>
            <Form.Item
              label="Admin email"
              name="admin_email"
              rules={[
                { type: "email", message: "Enter a valid email" },
              ]}
            >
              <Input autoComplete="email" placeholder="Optional" />
            </Form.Item>
            <Form.Item
              label="Admin password"
              name="admin_password"
              rules={[
                { required: true, message: "Password is required" },
                { min: 8, message: "Use at least 8 characters" },
              ]}
            >
              <Input.Password autoComplete="new-password" />
            </Form.Item>
            <Form.Item label="Site title" name="site_title">
              <Input placeholder="My Blog" />
            </Form.Item>
            <Form.Item label="Site subtitle" name="site_subtitle">
              <Input placeholder="Some witty tagline" />
            </Form.Item>
            <Form.Item label="Site base URL" name="site_base_url">
              <Input placeholder="https://blog.example.com" />
            </Form.Item>
            <Button type="primary" htmlType="submit" block loading={loading}>
              Create admin
            </Button>
          </Form>
        </Space>
      </Card>
    </div>
  );
}
