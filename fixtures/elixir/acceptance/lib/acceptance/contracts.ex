defmodule Acceptance.WorkerBehaviour do
  @callback run(term(), keyword()) :: {:ok, term()}
  @macrocallback build(term()) :: Macro.t()
  @optional_callbacks [build: 1]
end

defprotocol Acceptance.Renderable do
  def render(value)
end
