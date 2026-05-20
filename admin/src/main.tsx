import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { ConfigProvider, App as AntdApp } from "antd";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { router } from "@/router";
import { fetchSiteInfo } from "@/api/client";
import "antd/dist/reset.css";
import "@/styles/global.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});

function SiteTitle() {
  const site = useQuery({ queryKey: ["site-info"], queryFn: fetchSiteInfo });

  useEffect(() => {
    const title = site.data?.title?.trim();
    if (title) {
      document.title = title;
    }
  }, [site.data?.title]);

  return null;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ConfigProvider
      theme={{
        token: { colorPrimary: "#0b87fd", borderRadius: 6 },
      }}
    >
      <AntdApp>
        <QueryClientProvider client={queryClient}>
          <SiteTitle />
          <RouterProvider router={router} />
        </QueryClientProvider>
      </AntdApp>
    </ConfigProvider>
  </React.StrictMode>,
);
