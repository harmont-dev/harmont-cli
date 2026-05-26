"""COW workspace E2E fixture — three-step chain proving workspace inheritance."""
import harmont as hm


@hm.pipeline("cow-chain", default_image="alpine:latest")
def cow_chain():
    a = hm.sh("echo from-a > /workspace/a.txt", label="a")
    b = a.sh("cat /workspace/a.txt && echo from-b > /workspace/b.txt", label="b")
    c = b.sh("cat /workspace/a.txt && cat /workspace/b.txt && echo c-saw-both", label="c")
    return [c]
