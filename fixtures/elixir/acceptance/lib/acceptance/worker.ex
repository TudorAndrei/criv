defmodule Acceptance.Worker do
  @behaviour Acceptance.WorkerBehaviour

  alias Acceptance.Repo
  import Acceptance.Helpers, only: [tag: 1]
  require Acceptance.Macros, as: Macros
  use Acceptance.Dsl

  @spec run(term(), keyword()) :: {:ok, term()}
  def run(value, opts \\ [])
  def run(:skip, _opts), do: {:ok, :skipped}

  def run(value, opts) when is_list(opts) do
    tagged = tag(value)
    Macros.build(tagged)
    tagged |> Repo.fetch(opts)
  end

  defp hidden(value), do: value
  defmacro build(value), do: value
  defmacrop private_build(value), do: value
  defguard valid(value) when not is_nil(value)
  defguardp private_valid(value) when is_atom(value)
  defdelegate delegated(value), to: Repo, as: :fetch
  def capture_only(), do: &Repo.fetch/1
  def dynamic(fun, value), do: fun.(value)
  def literal_apply(value), do: apply(Repo, :fetch, [value])
end
