import { useEffect, useState } from "react";
import { Card, Form, Input, Button, Typography, App, Space } from "antd";
import { useNavigate } from "@tanstack/react-router";
import { login, fetchWhoAmI, fetchBootstrapStatus } from "@/api/client";
import { useAuthStore } from "@/state/auth";

const { Title, Paragraph, Link } = Typography;

export function LoginPage() {
  const { message } = App.useApp();
  const navigate = useNavigate();
  const setUser = useAuthStore((s) => s.setUser);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    fetchBootstrapStatus()
      .then((status) => {
        if (!status.bootstrapped) {
          navigate({ to: "/bootstrap" });
        }
      })
      .catch(() => {
        // Let the sign-in attempt surface the concrete API error.
      });
  }, [navigate]);

  const onFinish = async (values: { username: string; password: string }) => {
    setLoading(true);
    try {
      await login(values.username, values.password);
      const user = await fetchWhoAmI();
      setUser(user);
      void message.success(`Welcome back, ${user.display_name || user.name}`);
      navigate({ to: "/" });
    } catch (e) {
      void message.error(
        e instanceof Error ? e.message : "Sign in failed. Check your credentials.",
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="full-page-center">
      <Card style={{ maxWidth: 400, width: "100%" }}>
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <Title level={3} style={{ margin: 0 }}>
            Sign in to rblog
          </Title>
          <Paragraph type="secondary" style={{ margin: 0 }}>
            First time? <Link onClick={() => navigate({ to: "/bootstrap" })}>Run setup</Link>
          </Paragraph>
          <Form layout="vertical" onFinish={onFinish}>
            <Form.Item
              label="Username"
              name="username"
              rules={[{ required: true, message: "Username is required" }]}
            >
              <Input autoComplete="username" autoFocus />
            </Form.Item>
            <Form.Item
              label="Password"
              name="password"
              rules={[{ required: true, message: "Password is required" }]}
            >
              <Input.Password autoComplete="current-password" />
            </Form.Item>
            <Button type="primary" htmlType="submit" block loading={loading}>
              Sign in
            </Button>
          </Form>
        </Space>
      </Card>
    </div>
  );
}
