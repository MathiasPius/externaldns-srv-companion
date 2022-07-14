# externaldns-srv-companion
ExternalDNS companion container for automatically creating SRV records for NodePort services.

This container is designed to be used with the [Bitnami release](https://github.com/bitnami/charts/tree/master/bitnami/external-dns) of [ExternalDNS](https://github.com/kubernetes-sigs/external-dns)

# Usage
You can deploy this by defining it as a sidecar in your helm values, as below:

```yaml
sidecars:
- env:
  - name: RUST_LOG
    value: externaldns_srv_companion=DEBUG

  image: ghcr.io/mathiaspius/externaldns-srv-companion:0.1.1
  name: externaldns-srv-companion
  volumeMounts:
  - mountPath: '{{ .Values.aws.credentials.mountPath }}'
    name: aws-credentials
    readOnly: true
```

Since the container is created as a sidecar container to the external-dns container within the external-dns pod, it already shares the service account and can mount the AWS credentials from the external-dns container as seen above.

Note that this container does not follow the restrictions imposed on ExternalDNS, meaning any NodePort Service with the `external-dns.alpha.kubernetes.io/hostname` annotation will have an SRV record created, provided the hostname matches a hosted zone writeable by the provided AWS credentials.

# Example
To test the deployment, create an example pod and service as below
```yaml
kind: Pod
apiVersion: v1
metadata:
  name: apple-app
  labels:
    app: apple
spec:
  containers:
    - name: apple-app
      image: hashicorp/http-echo
      args:
        - "-text=apple"

---
kind: Service
apiVersion: v1
metadata:
  name: apple-service
  annotations:
    external-dns.alpha.kubernetes.io/hostname: apple.example.com.
spec:
  type: NodePort
  selector:
    app: apple
  ports:
    - name: apple-port
      port: 5678
```

And then verify that the SRV record has been created:

```shell
drill srv _apple-port._tcp.apple.example.com
```

Should yield something like:

```
_apple-port._tcp.apple.example.com.   1800    IN      SRV     0 10 31587 apple.example.com.
```
Where `31587` is the external NodePort assigned to the service, and `0` and `10` are hard-coded values for priority and weight respectively.