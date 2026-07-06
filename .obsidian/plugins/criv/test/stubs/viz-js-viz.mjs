export async function instance() {
  return {
    render() {
      return { status: "success", output: "<svg></svg>" };
    },
  };
}
